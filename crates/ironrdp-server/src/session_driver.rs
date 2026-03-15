//! Per-connection runtime for an accepted server session.
//!
//! This module owns the live client session state machine after transport
//! negotiation succeeds. Listener setup, security bootstrap, and channel
//! registration stay in [`crate::server`].

use std::rc::Rc;

use anyhow::{Context as _, Result, bail};
use ironrdp_acceptor::{Acceptor, AcceptorResult, DesktopSize};
use ironrdp_async::Framed;
use ironrdp_cliprdr::CliprdrServer;
use ironrdp_cliprdr::backend::ClipboardMessage;
use ironrdp_core::{decode, encode_vec};
use ironrdp_pdu::input::InputEventPdu;
use ironrdp_pdu::input::fast_path::{FastPathInput, FastPathInputEvent};
use ironrdp_pdu::mcs::{SendDataIndication, SendDataRequest};
use ironrdp_pdu::rdp::capability_sets::{BitmapCodecs, CapabilitySet, CmdFlags, CodecProperty, GeneralExtraFlags};
use ironrdp_pdu::rdp::headers::{ServerDeactivateAll, ShareControlPdu};
use ironrdp_pdu::x224::X224;
use ironrdp_pdu::{Action, mcs, rdp};
use ironrdp_svc::{ChannelFlags, StaticChannelId, SvcProcessor, server_encode_svc_messages};
use ironrdp_tokio::{FramedRead, FramedWrite, TokioFramed, split_tokio_framed, unsplit_tokio_framed};
use rdpsnd::server::{RdpsndServer, RdpsndServerMessage};
use tokio::sync::Mutex;
use tracing::{debug, error, trace, warn};
use {ironrdp_dvc as dvc, ironrdp_rdpsnd as rdpsnd};

use crate::display::DisplayUpdate;
use crate::echo::{EchoDvcBridge, EchoServerMessage, build_echo_request};
use crate::encoder::{UpdateEncoder, UpdateEncoderCodecs};
#[cfg(feature = "egfx")]
use crate::gfx::EgfxServerMessage;
use crate::server::{RdpServer, ServerEvent};

#[derive(Debug, PartialEq)]
pub(crate) enum RunState {
    Continue,
    Disconnect,
    DeactivationReactivation { desktop_size: DesktopSize },
}

enum DispatchDecision {
    Continue,
    Disconnect,
    Write(Vec<u8>),
}

#[expect(
    clippy::allow_attributes,
    reason = "clippy::multiple_inherent_impl is intentional here and cannot be attached with a fulfilled expect"
)]
#[allow(
    clippy::multiple_inherent_impl,
    reason = "server bootstrap and per-connection runtime stay split across modules on purpose"
)]
impl RdpServer {
    pub fn get_svc_processor<T: SvcProcessor + 'static>(&mut self) -> Option<&mut T> {
        self.static_channels
            .get_by_type_mut::<T>()
            .and_then(|svc| svc.channel_processor_downcast_mut())
    }

    pub fn get_channel_id_by_type<T: SvcProcessor + 'static>(&self) -> Option<StaticChannelId> {
        self.static_channels.get_channel_id_by_type::<T>()
    }

    async fn dispatch_pdu(
        &mut self,
        action: Action,
        bytes: bytes::BytesMut,
        writer: &mut impl FramedWrite,
        io_channel_id: u16,
        user_channel_id: u16,
    ) -> Result<RunState> {
        match action {
            Action::FastPath => {
                let input = decode(&bytes)?;
                self.handle_fastpath(input).await;
            }

            Action::X224 => {
                if self
                    .handle_x224(writer, io_channel_id, user_channel_id, &bytes)
                    .await
                    .context("X224 input error")?
                {
                    debug!("Got disconnect request");
                    return Ok(RunState::Disconnect);
                }
            }
        }

        Ok(RunState::Continue)
    }

    async fn dispatch_server_events(
        &mut self,
        events: &mut Vec<ServerEvent>,
        writer: &mut impl FramedWrite,
        user_channel_id: u16,
    ) -> Result<RunState> {
        // Avoid wave message queuing up and causing extra delays.
        // This is a naive solution, better solutions should compute the actual delay, add IO priority, encode audio, use UDP etc.
        // 4 frames should roughly corresponds to hundreds of ms in regular setups.
        let mut wave_limit = 4;
        for event in events.drain(..) {
            trace!(?event, "Dispatching");
            match self.dispatch_server_event(event, user_channel_id, &mut wave_limit)? {
                DispatchDecision::Continue => continue,
                DispatchDecision::Disconnect => return Ok(RunState::Disconnect),
                DispatchDecision::Write(data) => writer.write_all(&data).await?,
            }
        }

        Ok(RunState::Continue)
    }

    fn dispatch_server_event(
        &mut self,
        event: ServerEvent,
        user_channel_id: u16,
        wave_limit: &mut usize,
    ) -> Result<DispatchDecision> {
        match event {
            ServerEvent::Quit(reason) => {
                debug!("Got quit event: {reason}");
                Ok(DispatchDecision::Disconnect)
            }
            ServerEvent::GetLocalAddr(tx) => {
                let _ = tx.send(self.local_addr);
                Ok(DispatchDecision::Continue)
            }
            ServerEvent::SetCredentials(creds) => {
                self.set_credentials(Some(creds));
                Ok(DispatchDecision::Continue)
            }
            ServerEvent::Rdpsnd(msg) => self.dispatch_rdpsnd_event(msg, user_channel_id, wave_limit),
            ServerEvent::Clipboard(msg) => self.dispatch_clipboard_event(msg, user_channel_id),
            ServerEvent::Echo(msg) => self.dispatch_echo_event(msg, user_channel_id),
            #[cfg(feature = "egfx")]
            ServerEvent::Egfx(msg) => self.dispatch_egfx_event(msg, user_channel_id),
        }
    }

    fn dispatch_rdpsnd_event(
        &mut self,
        msg: RdpsndServerMessage,
        user_channel_id: u16,
        wave_limit: &mut usize,
    ) -> Result<DispatchDecision> {
        let Some(msgs) = ({
            let Some(rdpsnd) = self.get_svc_processor::<RdpsndServer>() else {
                warn!("No rdpsnd channel, dropping event");
                return Ok(DispatchDecision::Continue);
            };

            match msg {
                RdpsndServerMessage::Wave(data, ts) => {
                    if *wave_limit == 0 {
                        debug!("Dropping wave");
                        return Ok(DispatchDecision::Continue);
                    }
                    *wave_limit -= 1;
                    Some(rdpsnd.wave(data, ts))
                }
                RdpsndServerMessage::SetVolume { left, right } => Some(rdpsnd.set_volume(left, right)),
                RdpsndServerMessage::Close => Some(rdpsnd.close()),
                RdpsndServerMessage::Error(error) => {
                    error!(?error, "Handling rdpsnd event");
                    None
                }
            }
        }) else {
            return Ok(DispatchDecision::Continue);
        };

        let channel_id = self
            .get_channel_id_by_type::<RdpsndServer>()
            .context("SVC channel not found")?;
        let data = server_encode_svc_messages(
            msgs.context("failed to send rdpsnd event")?.into(),
            channel_id,
            user_channel_id,
        )?;

        Ok(DispatchDecision::Write(data))
    }

    fn dispatch_clipboard_event(&mut self, msg: ClipboardMessage, user_channel_id: u16) -> Result<DispatchDecision> {
        let Some(msgs) = ({
            let Some(cliprdr) = self.get_svc_processor::<CliprdrServer>() else {
                warn!("No clipboard channel, dropping event");
                return Ok(DispatchDecision::Continue);
            };

            match msg {
                ClipboardMessage::SendInitiateCopy(formats) => Some(cliprdr.initiate_copy(&formats)),
                ClipboardMessage::SendFormatData(data) => Some(cliprdr.submit_format_data(data)),
                ClipboardMessage::SendInitiatePaste(format) => Some(cliprdr.initiate_paste(format)),
                ClipboardMessage::SendLockClipboard { clip_data_id } => Some(cliprdr.lock_clipboard(clip_data_id)),
                ClipboardMessage::SendUnlockClipboard { clip_data_id } => Some(cliprdr.unlock_clipboard(clip_data_id)),
                ClipboardMessage::SendFileContentsRequest(request) => Some(cliprdr.request_file_contents(request)),
                ClipboardMessage::SendFileContentsResponse(response) => Some(cliprdr.submit_file_contents(response)),
                ClipboardMessage::Error(error) => {
                    error!(?error, "Handling clipboard event");
                    None
                }
            }
        }) else {
            return Ok(DispatchDecision::Continue);
        };

        let channel_id = self
            .get_channel_id_by_type::<CliprdrServer>()
            .context("SVC channel not found")?;
        let data = server_encode_svc_messages(
            msgs.context("failed to send clipboard event")?.into(),
            channel_id,
            user_channel_id,
        )?;

        Ok(DispatchDecision::Write(data))
    }

    fn dispatch_echo_event(&mut self, msg: EchoServerMessage, user_channel_id: u16) -> Result<DispatchDecision> {
        match msg {
            EchoServerMessage::SendRequest { payload } => {
                let Some(messages) = ({
                    let Some(drdynvc) = self.get_svc_processor::<dvc::DrdynvcServer>() else {
                        warn!("No drdynvc channel, dropping ECHO request");
                        return Ok(DispatchDecision::Continue);
                    };

                    let Some(echo_channel_id) = drdynvc.get_channel_id_by_type::<EchoDvcBridge>() else {
                        warn!("No ECHO dynamic channel, dropping ECHO request");
                        return Ok(DispatchDecision::Continue);
                    };

                    if !drdynvc.is_channel_opened(echo_channel_id) {
                        warn!("ECHO dynamic channel not yet opened, dropping ECHO request");
                        return Ok(DispatchDecision::Continue);
                    }

                    self.echo_handle.on_request_sent(&payload);
                    let request = build_echo_request(payload)?;

                    Some(dvc::encode_dvc_messages(
                        echo_channel_id,
                        vec![request],
                        ChannelFlags::SHOW_PROTOCOL,
                    )?)
                }) else {
                    return Ok(DispatchDecision::Continue);
                };

                let drdynvc_channel_id = self
                    .get_channel_id_by_type::<dvc::DrdynvcServer>()
                    .context("DRDYNVC channel not found")?;
                let data = server_encode_svc_messages(messages, drdynvc_channel_id, user_channel_id)?;

                Ok(DispatchDecision::Write(data))
            }
        }
    }

    #[cfg(feature = "egfx")]
    fn dispatch_egfx_event(&mut self, msg: EgfxServerMessage, user_channel_id: u16) -> Result<DispatchDecision> {
        match msg {
            EgfxServerMessage::SendMessages { messages } => {
                let drdynvc_channel_id = self
                    .get_channel_id_by_type::<dvc::DrdynvcServer>()
                    .context("DRDYNVC channel not found")?;
                let data = server_encode_svc_messages(messages, drdynvc_channel_id, user_channel_id)?;

                Ok(DispatchDecision::Write(data))
            }
        }
    }

    async fn client_loop<R, W>(
        &mut self,
        reader: &mut Framed<R>,
        writer: &mut Framed<W>,
        io_channel_id: u16,
        user_channel_id: u16,
        mut encoder: UpdateEncoder,
    ) -> Result<RunState>
    where
        R: FramedRead,
        W: FramedWrite,
    {
        debug!("Starting client loop");
        let mut display_updates = self.display.lock().await.updates().await?;
        let mut writer = SharedWriter::new(writer);
        let mut display_writer = writer.clone();
        let mut event_writer = writer.clone();
        let ev_receiver = std::sync::Arc::clone(&self.ev_receiver);
        let s = Rc::new(Mutex::new(self));

        let this = Rc::clone(&s);
        let dispatch_pdu = async move {
            loop {
                let (action, bytes) = reader.read_pdu().await?;
                let mut this = this.lock().await;
                match this
                    .dispatch_pdu(action, bytes, &mut writer, io_channel_id, user_channel_id)
                    .await?
                {
                    RunState::Continue => continue,
                    state => break Ok(state),
                }
            }
        };

        let dispatch_display = async move {
            let mut buffer = vec![0u8; 4096];

            loop {
                match display_updates.next_update().await {
                    Ok(Some(update)) => {
                        match Self::dispatch_display_update(
                            update,
                            &mut display_writer,
                            user_channel_id,
                            io_channel_id,
                            &mut buffer,
                            encoder,
                        )
                        .await?
                        {
                            (RunState::Continue, enc) => {
                                encoder = enc;
                                continue;
                            }
                            (state, _) => {
                                break Ok(state);
                            }
                        }
                    }
                    Ok(None) => {
                        break Ok(RunState::Disconnect);
                    }
                    Err(error) => {
                        break Err(error).context("display update stream failed");
                    }
                }
            }
        };

        let this = Rc::clone(&s);
        let mut ev_receiver = ev_receiver.lock().await;
        let dispatch_events = async move {
            let mut events = Vec::with_capacity(100);
            loop {
                let nevents = ev_receiver.recv_many(&mut events, 100).await;
                if nevents == 0 {
                    debug!("No sever events.. stopping");
                    break Ok(RunState::Disconnect);
                }
                while let Ok(ev) = ev_receiver.try_recv() {
                    events.push(ev);
                }
                let mut this = this.lock().await;
                match this
                    .dispatch_server_events(&mut events, &mut event_writer, user_channel_id)
                    .await?
                {
                    RunState::Continue => continue,
                    state => break Ok(state),
                }
            }
        };

        let state = tokio::select!(
            state = dispatch_pdu => state,
            state = dispatch_display => state,
            state = dispatch_events => state,
        );

        debug!("End of client loop: {state:?}");
        state
    }

    async fn client_accepted<R, W>(
        &mut self,
        reader: &mut Framed<R>,
        writer: &mut Framed<W>,
        result: AcceptorResult,
    ) -> Result<RunState>
    where
        R: FramedRead,
        W: FramedWrite,
    {
        debug!("Client accepted");

        if !result.input_events.is_empty() {
            debug!("Handling input event backlog from acceptor sequence");
            if self
                .handle_input_backlog(
                    writer,
                    result.io_channel_id,
                    result.user_channel_id,
                    result.input_events,
                )
                .await?
            {
                return Ok(RunState::Disconnect);
            }
        }

        self.static_channels = result.static_channels;
        if !result.reactivation {
            for (_type_id, channel, channel_id) in self.static_channels.iter_mut() {
                debug!(?channel, ?channel_id, "Start");
                let Some(channel_id) = channel_id else {
                    continue;
                };
                let svc_responses = channel.start()?;
                let response = server_encode_svc_messages(svc_responses, channel_id, result.user_channel_id)?;
                writer.write_all(&response).await?;
            }
        }

        let mut update_codecs = UpdateEncoderCodecs::new();
        let mut surface_flags = CmdFlags::empty();
        for c in result.capabilities {
            match c {
                CapabilitySet::General(c) => {
                    let fastpath = c.extra_flags.contains(GeneralExtraFlags::FASTPATH_OUTPUT_SUPPORTED);
                    if !fastpath {
                        bail!("Fastpath output not supported!");
                    }
                }
                CapabilitySet::Bitmap(b) => {
                    if !b.desktop_resize_flag {
                        debug!("Desktop resize is not supported by the client");
                        continue;
                    }

                    let client_size = DesktopSize {
                        width: b.desktop_width,
                        height: b.desktop_height,
                    };
                    let display_size = self.display.lock().await.request_initial_size(client_size).await;

                    // It's problematic when the client didn't resize, as we send bitmap updates that don't fit.
                    // The client will likely drop the connection.
                    if client_size.width < display_size.width || client_size.height < display_size.height {
                        // TODO: we may have different behaviour instead, such as clipping or scaling?
                        warn!(
                            "Client size doesn't fit the server size: {:?} < {:?}",
                            client_size, display_size
                        );
                    }
                }
                CapabilitySet::SurfaceCommands(c) => {
                    surface_flags = c.flags;
                }
                CapabilitySet::BitmapCodecs(BitmapCodecs(codecs)) => {
                    for codec in codecs {
                        match codec.property {
                            // FIXME: The encoder operates in image mode only.
                            //
                            // See [MS-RDPRFX] 3.1.1.1 "State Machine" for
                            // implementation of the video mode. which allows to
                            // skip sending Header for each image.
                            //
                            // We should distinguish parameters for both modes,
                            // and somehow choose the "best", instead of picking
                            // the last parsed here.
                            CodecProperty::RemoteFx(rdp::capability_sets::RemoteFxContainer::ClientContainer(c))
                                if self.opts.has_remote_fx() =>
                            {
                                for caps in c.caps_data.0.0 {
                                    update_codecs.set_remotefx(Some((caps.entropy_bits, codec.id)));
                                }
                            }
                            CodecProperty::ImageRemoteFx(rdp::capability_sets::RemoteFxContainer::ClientContainer(
                                c,
                            )) if self.opts.has_image_remote_fx() => {
                                for caps in c.caps_data.0.0 {
                                    update_codecs.set_remotefx(Some((caps.entropy_bits, codec.id)));
                                }
                            }
                            CodecProperty::NsCodec(_) => (),
                            #[cfg(feature = "qoi")]
                            CodecProperty::Qoi if self.opts.has_qoi() => {
                                update_codecs.set_qoi(Some(codec.id));
                            }
                            #[cfg(feature = "qoiz")]
                            CodecProperty::QoiZ if self.opts.has_qoiz() => {
                                update_codecs.set_qoiz(Some(codec.id));
                            }
                            _ => (),
                        }
                    }
                }
                _ => {}
            }
        }

        let desktop_size = self.display.lock().await.size().await;
        let encoder = UpdateEncoder::new(desktop_size, surface_flags, update_codecs, self.opts.max_request_size)
            .context("failed to initialize update encoder")?;

        let state = self
            .client_loop(reader, writer, result.io_channel_id, result.user_channel_id, encoder)
            .await
            .context("client loop failure")?;

        Ok(state)
    }

    async fn handle_input_backlog(
        &mut self,
        writer: &mut impl FramedWrite,
        io_channel_id: u16,
        user_channel_id: u16,
        frames: Vec<Vec<u8>>,
    ) -> Result<bool> {
        for frame in frames {
            match Action::from_fp_output_header(frame[0]) {
                Ok(Action::FastPath) => {
                    let input = decode(&frame)?;
                    self.handle_fastpath(input).await;
                }

                Ok(Action::X224) => {
                    if self.handle_x224(writer, io_channel_id, user_channel_id, &frame).await? {
                        return Ok(true);
                    }
                }

                // the frame here is always valid, because otherwise it would
                // have failed during the acceptor loop
                Err(_) => unreachable!(),
            }
        }

        Ok(false)
    }

    async fn handle_fastpath(&mut self, input: FastPathInput) {
        let mut handler = self.handler.lock().await;
        for event in input.input_events().iter().copied() {
            match event {
                FastPathInputEvent::KeyboardEvent(flags, key) => {
                    handler.keyboard((key, flags).into());
                }

                FastPathInputEvent::UnicodeKeyboardEvent(flags, key) => {
                    handler.keyboard((key, flags).into());
                }

                FastPathInputEvent::SyncEvent(flags) => {
                    handler.keyboard(flags.into());
                }

                FastPathInputEvent::MouseEvent(mouse) => {
                    handler.mouse(mouse.into());
                }

                FastPathInputEvent::MouseEventEx(mouse) => {
                    handler.mouse(mouse.into());
                }

                FastPathInputEvent::MouseEventRel(mouse) => {
                    handler.mouse(mouse.into());
                }

                FastPathInputEvent::QoeEvent(quality) => {
                    warn!("Received QoE: {}", quality);
                }
            }
        }
    }

    async fn handle_io_channel_data(&mut self, data: SendDataRequest<'_>) -> Result<bool> {
        let control: rdp::headers::ShareControlHeader = decode(data.user_data.as_ref())?;

        match control.share_control_pdu {
            ShareControlPdu::Data(header) => match header.share_data_pdu {
                rdp::headers::ShareDataPdu::Input(pdu) => {
                    self.handle_input_event(pdu).await;
                }

                rdp::headers::ShareDataPdu::ShutdownRequest => {
                    return Ok(true);
                }

                unexpected => {
                    warn!(?unexpected, "Unexpected share data pdu");
                }
            },

            unexpected => {
                warn!(?unexpected, "Unexpected share control");
            }
        }

        Ok(false)
    }

    async fn handle_x224(
        &mut self,
        writer: &mut impl FramedWrite,
        io_channel_id: u16,
        user_channel_id: u16,
        frame: &[u8],
    ) -> Result<bool> {
        let message = decode::<X224<mcs::McsMessage<'_>>>(frame)?;
        match message.0 {
            mcs::McsMessage::SendDataRequest(data) => {
                debug!(?data, "McsMessage::SendDataRequest");
                if data.channel_id == io_channel_id {
                    return self.handle_io_channel_data(data).await;
                }

                if let Some(svc) = self.static_channels.get_by_channel_id_mut(data.channel_id) {
                    let response_pdus = svc.process(&data.user_data)?;
                    let response = server_encode_svc_messages(response_pdus, data.channel_id, user_channel_id)?;
                    writer.write_all(&response).await?;
                } else {
                    warn!(channel_id = data.channel_id, "Unexpected channel received: ID",);
                }
            }

            mcs::McsMessage::DisconnectProviderUltimatum(disconnect) => {
                if disconnect.reason == mcs::DisconnectReason::UserRequested {
                    return Ok(true);
                }
            }

            _ => {
                warn!(name = ironrdp_core::name(&message), "Unexpected mcs message");
            }
        }

        Ok(false)
    }

    async fn handle_input_event(&mut self, input: InputEventPdu) {
        let mut handler = self.handler.lock().await;
        for event in input.0 {
            match event {
                ironrdp_pdu::input::InputEvent::ScanCode(key) => {
                    handler.keyboard((key.key_code, key.flags).into());
                }

                ironrdp_pdu::input::InputEvent::Unicode(key) => {
                    handler.keyboard((key.unicode_code, key.flags).into());
                }

                ironrdp_pdu::input::InputEvent::Sync(sync) => {
                    handler.keyboard(sync.flags.into());
                }

                ironrdp_pdu::input::InputEvent::Mouse(mouse) => {
                    handler.mouse(mouse.into());
                }

                ironrdp_pdu::input::InputEvent::MouseX(mouse) => {
                    handler.mouse(mouse.into());
                }

                ironrdp_pdu::input::InputEvent::MouseRel(mouse) => {
                    handler.mouse(mouse.into());
                }

                ironrdp_pdu::input::InputEvent::Unused(_) => {}
            }
        }
    }

    pub(crate) async fn accept_finalize<S>(
        &mut self,
        mut framed: TokioFramed<S>,
        mut acceptor: Acceptor,
    ) -> Result<TokioFramed<S>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Sync + Send + Unpin,
    {
        loop {
            let (new_framed, result) = ironrdp_acceptor::accept_finalize(framed, &mut acceptor)
                .await
                .context("failed to accept client during finalize")?;

            let (mut reader, mut writer) = split_tokio_framed(new_framed);

            match self.client_accepted(&mut reader, &mut writer, result).await? {
                RunState::Continue => {
                    unreachable!();
                }
                RunState::DeactivationReactivation { desktop_size } => {
                    // No description of such behavior was found in the
                    // specification, but apparently, we must keep the channel
                    // state as they were during reactivation. This fixes
                    // various state issues during client resize.
                    acceptor = Acceptor::new_deactivation_reactivation(
                        acceptor,
                        core::mem::take(&mut self.static_channels),
                        desktop_size,
                    )?;
                    framed = unsplit_tokio_framed(reader, writer);
                    continue;
                }
                RunState::Disconnect => {
                    let final_framed = unsplit_tokio_framed(reader, writer);
                    return Ok(final_framed);
                }
            }
        }
    }

    async fn dispatch_display_update(
        update: DisplayUpdate,
        writer: &mut impl FramedWrite,
        user_channel_id: u16,
        io_channel_id: u16,
        buffer: &mut Vec<u8>,
        mut encoder: UpdateEncoder,
    ) -> Result<(RunState, UpdateEncoder)> {
        if let DisplayUpdate::Resize(desktop_size) = update {
            debug!(?desktop_size, "Display resize");
            encoder.set_desktop_size(desktop_size);
            deactivate_all(io_channel_id, user_channel_id, writer).await?;
            return Ok((RunState::DeactivationReactivation { desktop_size }, encoder));
        }

        let mut encoder_iter = encoder.update(update);
        loop {
            let Some(fragmenter) = encoder_iter.next().await else {
                break;
            };

            let mut fragmenter = fragmenter.context("error while encoding")?;
            if fragmenter.size_hint() > buffer.len() {
                buffer.resize(fragmenter.size_hint(), 0);
            }

            while let Some(len) = fragmenter.next(buffer) {
                writer
                    .write_all(&buffer[..len])
                    .await
                    .context("failed to write display update")?;
            }
        }

        Ok((RunState::Continue, encoder))
    }
}

async fn deactivate_all(io_channel_id: u16, user_channel_id: u16, writer: &mut impl FramedWrite) -> Result<()> {
    let pdu = ShareControlPdu::ServerDeactivateAll(ServerDeactivateAll);
    let pdu = rdp::headers::ShareControlHeader {
        share_id: 0,
        pdu_source: io_channel_id,
        share_control_pdu: pdu,
    };
    let user_data = encode_vec(&pdu)?.into();
    let pdu = SendDataIndication {
        initiator_id: user_channel_id,
        channel_id: io_channel_id,
        user_data,
    };
    let msg = encode_vec(&X224(pdu))?;
    writer.write_all(&msg).await?;
    Ok(())
}

struct SharedWriter<'w, W: FramedWrite> {
    writer: Rc<Mutex<&'w mut W>>,
}

impl<W: FramedWrite> Clone for SharedWriter<'_, W> {
    fn clone(&self) -> Self {
        Self {
            writer: Rc::clone(&self.writer),
        }
    }
}

impl<W> FramedWrite for SharedWriter<'_, W>
where
    W: FramedWrite,
{
    type WriteAllFut<'write>
        = core::pin::Pin<Box<dyn Future<Output = std::io::Result<()>> + 'write>>
    where
        Self: 'write;

    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteAllFut<'a> {
        Box::pin(async {
            let mut writer = self.writer.lock().await;

            writer.write_all(buf).await?;
            Ok(())
        })
    }
}

impl<'a, W: FramedWrite> SharedWriter<'a, W> {
    fn new(writer: &'a mut W) -> Self {
        Self {
            writer: Rc::new(Mutex::new(writer)),
        }
    }
}
