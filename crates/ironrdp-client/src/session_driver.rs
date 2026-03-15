//! Active-session runtime for the native client.
//!
//! This module owns the post-connect session loop: it translates transport frames
//! into [`ActiveStage`] updates, turns GUI-originated input into protocol output,
//! and forwards rendered image and pointer updates back to the window event loop.
//! `rdp.rs` keeps connection establishment and reconnect policy, while this module
//! owns the live-session driver and the reusable packed-frame buffer used by the
//! software presentation path.

use core::num::NonZeroU16;

use ironrdp::cliprdr;
use ironrdp::cliprdr::backend::ClipboardMessage;
use ironrdp::connector::ConnectionResult;
use ironrdp::connector::connection_activation::{ConnectionActivationSequence, ConnectionActivationState};
use ironrdp::displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{self, ActiveStage, ActiveStageOutput, GracefulDisconnectReason, SessionResult, fast_path};
use ironrdp_core::WriteBuf;
use ironrdp_tokio::{FramedWrite as _, TokioFramed, single_sequence_step_read, split_tokio_framed};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};
use winit::event_loop::EventLoopProxy;

use crate::rdp::{RdpInputEvent, RdpOutputEvent};

pub(crate) enum RdpControlFlow {
    ReconnectWithNewSize { width: u16, height: u16 },
    TerminatedGracefully(GracefulDisconnectReason),
}

enum SessionDriverFlow {
    Outputs(Vec<ActiveStageOutput>),
    ReconnectWithNewSize { width: u16, height: u16 },
}

struct SessionDriver {
    image: DecodedImage,
    active_stage: ActiveStage,
    reusable_frame_buffer: Vec<u32>,
}

impl SessionDriver {
    fn new(connection_result: ConnectionResult) -> Self {
        let image = DecodedImage::new(
            PixelFormat::RgbA32,
            connection_result.desktop_size.width,
            connection_result.desktop_size.height,
        );

        let active_stage = ActiveStage::new(connection_result);

        Self {
            image,
            active_stage,
            reusable_frame_buffer: Vec::new(),
        }
    }

    fn process_server_frame(
        &mut self,
        action: ironrdp::pdu::Action,
        payload: &[u8],
    ) -> SessionResult<Vec<ActiveStageOutput>> {
        self.active_stage.process(&mut self.image, action, payload)
    }

    fn handle_input_event(&mut self, input_event: RdpInputEvent) -> SessionResult<SessionDriverFlow> {
        match input_event {
            RdpInputEvent::Resize {
                width,
                height,
                scale_factor,
                physical_size,
            } => {
                trace!(width, height, "Resize event");
                let width = u32::from(width);
                let height = u32::from(height);
                let (width, height) = MonitorLayoutEntry::adjust_display_size(width, height);
                debug!(width, height, "Adjusted display size");

                if let Some(response_frame) =
                    self.active_stage
                        .encode_resize(width, height, Some(scale_factor), physical_size)
                {
                    Ok(SessionDriverFlow::Outputs(vec![ActiveStageOutput::ResponseFrame(
                        response_frame?,
                    )]))
                } else {
                    debug!("Reconnecting with new size");
                    let width = u16::try_from(width).expect("always in the range");
                    let height = u16::try_from(height).expect("always in the range");
                    Ok(SessionDriverFlow::ReconnectWithNewSize { width, height })
                }
            }
            RdpInputEvent::FastPath(events) => {
                trace!(?events);
                Ok(SessionDriverFlow::Outputs(
                    self.active_stage.process_fastpath_input(&mut self.image, &events)?,
                ))
            }
            RdpInputEvent::Close => Ok(SessionDriverFlow::Outputs(self.active_stage.graceful_shutdown()?)),
            RdpInputEvent::Clipboard(event) => {
                if let Some(cliprdr) = self.active_stage.get_svc_processor::<cliprdr::CliprdrClient>() {
                    if let Some(svc_messages) = match event {
                        ClipboardMessage::SendInitiateCopy(formats) => Some(
                            cliprdr
                                .initiate_copy(&formats)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::SendFormatData(response) => Some(
                            cliprdr
                                .submit_format_data(response)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::SendInitiatePaste(format) => Some(
                            cliprdr
                                .initiate_paste(format)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::SendLockClipboard { clip_data_id } => Some(
                            cliprdr
                                .lock_clipboard(clip_data_id)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::SendUnlockClipboard { clip_data_id } => Some(
                            cliprdr
                                .unlock_clipboard(clip_data_id)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::SendFileContentsRequest(request) => Some(
                            cliprdr
                                .request_file_contents(request)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::SendFileContentsResponse(response) => Some(
                            cliprdr
                                .submit_file_contents(response)
                                .map_err(|e| session::custom_err!("CLIPRDR", e))?,
                        ),
                        ClipboardMessage::Error(error) => {
                            error!("Clipboard backend error: {}", error);
                            None
                        }
                    } {
                        let frame = self.active_stage.process_svc_processor_messages(svc_messages)?;
                        Ok(SessionDriverFlow::Outputs(vec![ActiveStageOutput::ResponseFrame(
                            frame,
                        )]))
                    } else {
                        Ok(SessionDriverFlow::Outputs(Vec::new()))
                    }
                } else {
                    warn!("Clipboard event received, but Cliprdr is not available");
                    Ok(SessionDriverFlow::Outputs(Vec::new()))
                }
            }
            RdpInputEvent::RecycleFrameBuffer(mut buffer) => {
                buffer.clear();
                if buffer.capacity() >= self.reusable_frame_buffer.capacity() {
                    self.reusable_frame_buffer = buffer;
                }

                Ok(SessionDriverFlow::Outputs(Vec::new()))
            }
            RdpInputEvent::SendDvcMessages { channel_id, messages } => {
                trace!(channel_id, ?messages, "Send DVC messages");
                let frame = self.active_stage.encode_dvc_messages(messages)?;
                Ok(SessionDriverFlow::Outputs(vec![ActiveStageOutput::ResponseFrame(
                    frame,
                )]))
            }
        }
    }

    async fn handle_stage_outputs<R, W>(
        &mut self,
        reader: &mut TokioFramed<R>,
        writer: &mut TokioFramed<W>,
        event_loop_proxy: &EventLoopProxy<RdpOutputEvent>,
        outputs: Vec<ActiveStageOutput>,
    ) -> SessionResult<Option<GracefulDisconnectReason>>
    where
        R: AsyncRead + Unpin + Send + Sync,
        W: AsyncWrite + Unpin + Send + Sync,
    {
        for out in outputs {
            if let Some(reason) = self.handle_stage_output(reader, writer, event_loop_proxy, out).await? {
                return Ok(Some(reason));
            }
        }

        Ok(None)
    }

    async fn handle_stage_output<R, W>(
        &mut self,
        reader: &mut TokioFramed<R>,
        writer: &mut TokioFramed<W>,
        event_loop_proxy: &EventLoopProxy<RdpOutputEvent>,
        out: ActiveStageOutput,
    ) -> SessionResult<Option<GracefulDisconnectReason>>
    where
        R: AsyncRead + Unpin + Send + Sync,
        W: AsyncWrite + Unpin + Send + Sync,
    {
        match out {
            ActiveStageOutput::ResponseFrame(frame) => {
                writer
                    .write_all(&frame)
                    .await
                    .map_err(|e| session::custom_err!("write response", e))?;
                Ok(None)
            }
            ActiveStageOutput::GraphicsUpdate(_region) => {
                self.emit_image_update(event_loop_proxy)?;
                Ok(None)
            }
            ActiveStageOutput::PointerDefault => {
                event_loop_proxy
                    .send_event(RdpOutputEvent::PointerDefault)
                    .map_err(|e| session::custom_err!("event_loop_proxy", e))?;
                Ok(None)
            }
            ActiveStageOutput::PointerHidden => {
                event_loop_proxy
                    .send_event(RdpOutputEvent::PointerHidden)
                    .map_err(|e| session::custom_err!("event_loop_proxy", e))?;
                Ok(None)
            }
            ActiveStageOutput::PointerPosition { x, y } => {
                event_loop_proxy
                    .send_event(RdpOutputEvent::PointerPosition { x, y })
                    .map_err(|e| session::custom_err!("event_loop_proxy", e))?;
                Ok(None)
            }
            ActiveStageOutput::PointerBitmap(pointer) => {
                event_loop_proxy
                    .send_event(RdpOutputEvent::PointerBitmap(pointer))
                    .map_err(|e| session::custom_err!("event_loop_proxy", e))?;
                Ok(None)
            }
            ActiveStageOutput::DeactivateAll(connection_activation) => {
                self.handle_deactivation_reactivation(reader, writer, connection_activation)
                    .await?;
                Ok(None)
            }
            ActiveStageOutput::MultitransportRequest(pdu) => {
                debug!(
                    request_id = pdu.request_id,
                    requested_protocol = ?pdu.requested_protocol,
                    "Multitransport request received (UDP transport not implemented)"
                );
                Ok(None)
            }
            ActiveStageOutput::Terminate(reason) => Ok(Some(reason)),
        }
    }

    fn emit_image_update(&mut self, event_loop_proxy: &EventLoopProxy<RdpOutputEvent>) -> SessionResult<()> {
        let mut buffer = core::mem::take(&mut self.reusable_frame_buffer);
        pack_rgba_frame(self.image.data(), &mut buffer)?;

        event_loop_proxy
            .send_event(RdpOutputEvent::Image {
                buffer,
                width: NonZeroU16::new(self.image.width()).ok_or_else(|| session::general_err!("width is zero"))?,
                height: NonZeroU16::new(self.image.height()).ok_or_else(|| session::general_err!("height is zero"))?,
            })
            .map_err(|e| session::custom_err!("event_loop_proxy", e))?;

        Ok(())
    }

    async fn handle_deactivation_reactivation<R, W>(
        &mut self,
        reader: &mut TokioFramed<R>,
        writer: &mut TokioFramed<W>,
        mut connection_activation: Box<ConnectionActivationSequence>,
    ) -> SessionResult<()>
    where
        R: AsyncRead + Unpin + Send + Sync,
        W: AsyncWrite + Unpin + Send + Sync,
    {
        debug!("Received Server Deactivate All PDU, executing Deactivation-Reactivation Sequence");
        let mut buf = WriteBuf::new();

        loop {
            let written = single_sequence_step_read(reader, &mut *connection_activation, &mut buf)
                .await
                .map_err(|e| session::custom_err!("read deactivation-reactivation sequence step", e))?;

            if written.size().is_some() {
                writer
                    .write_all(buf.filled())
                    .await
                    .map_err(|e| session::custom_err!("write deactivation-reactivation sequence step", e))?;
            }

            if let ConnectionActivationState::Finalized {
                io_channel_id,
                user_channel_id,
                desktop_size,
                share_id,
                enable_server_pointer,
                pointer_software_rendering,
            } = connection_activation.connection_activation_state()
            {
                debug!(?desktop_size, "Deactivation-Reactivation Sequence completed");
                self.image = DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
                self.active_stage.set_fastpath_processor(
                    fast_path::ProcessorBuilder {
                        io_channel_id,
                        user_channel_id,
                        share_id,
                        enable_server_pointer,
                        pointer_software_rendering,
                        bulk_decompressor: None,
                    }
                    .build(),
                );
                self.active_stage.set_share_id(share_id);
                self.active_stage.set_enable_server_pointer(enable_server_pointer);
                return Ok(());
            }
        }
    }
}

pub(crate) async fn run_active_session<S>(
    framed: TokioFramed<S>,
    connection_result: ConnectionResult,
    event_loop_proxy: &EventLoopProxy<RdpOutputEvent>,
    input_event_receiver: &mut mpsc::UnboundedReceiver<RdpInputEvent>,
) -> SessionResult<RdpControlFlow>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + Sync,
{
    let (mut reader, mut writer) = split_tokio_framed(framed);
    let mut driver = SessionDriver::new(connection_result);

    let disconnect_reason = 'outer: loop {
        let flow = tokio::select! {
            frame = reader.read_pdu() => {
                let (action, payload) = frame.map_err(|e| session::custom_err!("read frame", e))?;
                trace!(?action, frame_length = payload.len(), "Frame received");
                SessionDriverFlow::Outputs(driver.process_server_frame(action, &payload)?)
            }
            input_event = input_event_receiver.recv() => {
                let input_event = input_event.ok_or_else(|| session::general_err!("GUI is stopped"))?;
                driver.handle_input_event(input_event)?
            }
        };

        match flow {
            SessionDriverFlow::Outputs(outputs) => {
                if let Some(reason) = driver
                    .handle_stage_outputs(&mut reader, &mut writer, event_loop_proxy, outputs)
                    .await?
                {
                    break 'outer reason;
                }
            }
            SessionDriverFlow::ReconnectWithNewSize { width, height } => {
                return Ok(RdpControlFlow::ReconnectWithNewSize { width, height });
            }
        }
    };

    Ok(RdpControlFlow::TerminatedGracefully(disconnect_reason))
}

fn pack_rgba_frame(image_data: &[u8], buffer: &mut Vec<u32>) -> SessionResult<()> {
    let mut pixels = image_data.chunks_exact(4);
    buffer.clear();
    buffer.extend(pixels.by_ref().map(|pixel| {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        u32::from_be_bytes([0, r, g, b])
    }));

    if !pixels.remainder().is_empty() {
        return Err(session::general_err!("decoded image length is not divisible by four"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::pack_rgba_frame;

    #[test]
    fn pack_rgba_frame_converts_pixels_for_softbuffer() {
        let image = [
            0x11, 0x22, 0x33, 0xff, //
            0x44, 0x55, 0x66, 0x77,
        ];
        let mut buffer = Vec::new();

        pack_rgba_frame(&image, &mut buffer).expect("pack frame");

        assert_eq!(buffer, vec![0x0011_2233, 0x0044_5566]);
    }

    #[test]
    fn pack_rgba_frame_reuses_existing_capacity() {
        let image = [
            0x01, 0x02, 0x03, 0xff, //
            0x04, 0x05, 0x06, 0xff,
        ];
        let mut buffer = Vec::with_capacity(8);
        let initial_capacity = buffer.capacity();

        pack_rgba_frame(&image, &mut buffer).expect("pack frame");

        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.capacity(), initial_capacity);
    }
}
