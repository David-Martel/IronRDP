//! Active-session runtime for the native client.
//!
//! This module owns the post-connect session loop: it translates transport frames
//! into [`ActiveStage`] updates, turns GUI-originated input into protocol output,
//! and forwards rendered image and pointer updates back to the window event loop.
//! `rdp.rs` keeps connection establishment and reconnect policy, while this module
//! owns the live-session driver and the reusable RGBA frame buffer used by the
//! software presentation path. It also emits lightweight frame-copy and reconnect
//! diagnostics to guide the next render and transport optimization passes.
//! Multitransport requests are currently answered explicitly with `E_ABORT` on the
//! TCP control path so negotiation remains standards-complete until a real UDP
//! sideband transport is implemented.

use core::num::NonZeroU16;
use core::time::Duration;
use std::time::Instant;
use tokio::time::{self as tokio_time, Instant as TokioInstant};

use ironrdp::cliprdr;
use ironrdp::cliprdr::backend::ClipboardMessage;
use ironrdp::connector::ConnectionResult;
use ironrdp::connector::connection_activation::{ConnectionActivationSequence, ConnectionActivationState};
use ironrdp::displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::pdu::geometry::InclusiveRectangle;
use ironrdp::pdu::rdp::multitransport::MultitransportResponsePdu;
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{self, ActiveStage, ActiveStageOutput, GracefulDisconnectReason, SessionResult};
use ironrdp_core::WriteBuf;
use ironrdp_tokio::{FramedWrite as _, TokioFramed, single_sequence_step_read, split_tokio_framed};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};
use winit::event_loop::EventLoopProxy;

use crate::rdp::{RdpInputEvent, RdpOutputEvent};

/// Minimum interval between successive frame emissions when a new frame arrives
/// while a prior frame is still in flight.  Set to 4 ms (~250 fps cap) so the
/// server-update burst can be coalesced without starving the display pipeline.
const FRAME_PACING_INTERVAL: Duration = Duration::from_millis(4);

pub(crate) enum RdpControlFlow {
    ReconnectWithNewSize { width: u16, height: u16 },
    TerminatedGracefully(GracefulDisconnectReason),
}

enum SessionDriverFlow {
    Outputs(Vec<ActiveStageOutput>),
    EmitLatestImage,
    ReconnectWithNewSize { width: u16, height: u16 },
}

struct SessionDriver {
    image: DecodedImage,
    active_stage: ActiveStage,
    reusable_frame_buffer: Vec<u8>,
    frame_in_flight: bool,
    latest_frame_dirty: bool,
    emitted_frame_count: u64,
    reconnect_resize_count: u64,
    /// When set, a pacing timer is pending.  The session driver will wait for this
    /// instant before emitting the next dirty frame, coalescing server updates.
    frame_pacing_deadline: Option<TokioInstant>,
    /// Accumulated dirty region across all graphics updates coalesced into the
    /// next frame emission.  Stored as the axis-aligned bounding box of every
    /// rectangle received since the last frame was emitted.  `None` means no
    /// update has been recorded yet, or the previous frame was a full copy (e.g.
    /// after a resize), so the next emission must also copy the full frame.
    dirty_region: Option<InclusiveRectangle>,
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
            frame_in_flight: false,
            latest_frame_dirty: false,
            emitted_frame_count: 0,
            reconnect_resize_count: 0,
            frame_pacing_deadline: None,
            dirty_region: None,
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
                    self.reconnect_resize_count = self.reconnect_resize_count.saturating_add(1);
                    debug!(
                        reconnect_resize_count = self.reconnect_resize_count,
                        width, height, "Resize requires reconnect"
                    );
                    Ok(SessionDriverFlow::ReconnectWithNewSize { width, height })
                }
            }
            RdpInputEvent::FastPath(events) => {
                trace!(?events);
                Ok(SessionDriverFlow::Outputs(
                    self.active_stage.process_fastpath_input(&mut self.image, &events)?,
                ))
            }
            RdpInputEvent::FramePresented => {
                if finish_frame_present(&mut self.frame_in_flight, &mut self.latest_frame_dirty) {
                    // Defer emission by a small pacing interval to coalesce bursts.
                    // Only arm the timer if one is not already pending.
                    if self.frame_pacing_deadline.is_none() {
                        self.frame_pacing_deadline = Some(TokioInstant::now() + FRAME_PACING_INTERVAL);
                    }
                }
                Ok(SessionDriverFlow::Outputs(Vec::new()))
            }
            RdpInputEvent::Close => {
                info!("User-initiated disconnect: sending graceful shutdown to server");
                Ok(SessionDriverFlow::Outputs(self.active_stage.graceful_shutdown()?))
            }
            RdpInputEvent::Clipboard(event) => {
                trace!(
                    message_kind = clipboard_message_kind(&event),
                    "Handling clipboard event"
                );
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
        let mut graphics_update_pending = false;

        for out in outputs {
            if let ActiveStageOutput::GraphicsUpdate(region) = out {
                // Coalesce consecutive updates into an axis-aligned bounding box
                // stored on `self`.  The actual copy happens in `emit_image_update`.
                union_dirty_region(&mut self.dirty_region, region);
                graphics_update_pending = true;
                continue;
            }

            if graphics_update_pending {
                self.queue_latest_image_update(event_loop_proxy)?;
                graphics_update_pending = false;
            }

            if let Some(reason) = self.handle_stage_output(reader, writer, event_loop_proxy, out).await? {
                return Ok(Some(reason));
            }
        }

        if graphics_update_pending {
            self.queue_latest_image_update(event_loop_proxy)?;
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
            ActiveStageOutput::GraphicsUpdate(region) => {
                union_dirty_region(&mut self.dirty_region, region);
                self.queue_latest_image_update(event_loop_proxy)?;
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
                let response = self
                    .active_stage
                    .encode_multitransport_response(&MultitransportResponsePdu::abort(pdu.request_id))?;
                writer
                    .write_all(&response)
                    .await
                    .map_err(|e| session::custom_err!("write multitransport response", e))?;
                debug!(
                    request_id = pdu.request_id,
                    requested_protocol = ?pdu.requested_protocol,
                    response = "E_ABORT",
                    "Multitransport request received (UDP transport not implemented)"
                );
                Ok(None)
            }
            ActiveStageOutput::Terminate(reason) => {
                info!(%reason, "Server-initiated graceful disconnect received");
                Ok(Some(reason))
            }
        }
    }

    fn emit_image_update(&mut self, event_loop_proxy: &EventLoopProxy<RdpOutputEvent>) -> SessionResult<()> {
        let started_at = Instant::now();
        let dirty = self.dirty_region.take();
        let mut buffer = core::mem::take(&mut self.reusable_frame_buffer);
        let copy_started_at = Instant::now();
        let partial = copy_rgba_frame(self.image.data(), &mut buffer, self.image.width(), dirty.as_ref())?;
        let copy_duration = copy_started_at.elapsed();
        let width = NonZeroU16::new(self.image.width()).ok_or_else(|| session::general_err!("width is zero"))?;
        let height = NonZeroU16::new(self.image.height()).ok_or_else(|| session::general_err!("height is zero"))?;

        event_loop_proxy
            .send_event(RdpOutputEvent::Image { buffer, width, height })
            .map_err(|e| session::custom_err!("event_loop_proxy", e))?;

        self.emitted_frame_count = self.emitted_frame_count.saturating_add(1);
        if self.emitted_frame_count == 1 {
            info!(width = width.get(), height = height.get(), "First image update emitted");
        }
        trace!(
            frame_id = self.emitted_frame_count,
            width = width.get(),
            height = height.get(),
            pixels = usize::from(width.get()) * usize::from(height.get()),
            partial_copy = partial,
            copy_micros = copy_duration.as_micros(),
            total_micros = started_at.elapsed().as_micros(),
            "Emitted image update"
        );

        Ok(())
    }

    fn queue_latest_image_update(&mut self, event_loop_proxy: &EventLoopProxy<RdpOutputEvent>) -> SessionResult<()> {
        if !should_emit_latest_image(self.frame_in_flight, &mut self.latest_frame_dirty) {
            trace!(
                pending_frame = self.frame_in_flight,
                emitted_frame_count = self.emitted_frame_count,
                "Deferring image emit while the previous frame is still pending"
            );
            return Ok(());
        }

        self.emit_image_update(event_loop_proxy)?;
        self.frame_in_flight = true;
        self.latest_frame_dirty = false;

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
                // Any accumulated dirty region from the prior image is now invalid;
                // the next frame emission must copy the full (resized) frame.
                self.dirty_region = None;
                self.active_stage.reactivate_fastpath_processor(
                    io_channel_id,
                    user_channel_id,
                    share_id,
                    enable_server_pointer,
                    pointer_software_rendering,
                );
                self.active_stage.set_share_id(share_id);
                self.active_stage.set_enable_server_pointer(enable_server_pointer);
                return Ok(());
            }
        }
    }
}

/// Expand `accumulated` to be the axis-aligned bounding box of itself and
/// `new_rect`.  If `accumulated` is `None`, it is set to `new_rect`.
fn union_dirty_region(accumulated: &mut Option<InclusiveRectangle>, new_rect: InclusiveRectangle) {
    match accumulated {
        None => *accumulated = Some(new_rect),
        Some(existing) => {
            existing.left = existing.left.min(new_rect.left);
            existing.top = existing.top.min(new_rect.top);
            existing.right = existing.right.max(new_rect.right);
            existing.bottom = existing.bottom.max(new_rect.bottom);
        }
    }
}

fn should_emit_latest_image(frame_in_flight: bool, latest_frame_dirty: &mut bool) -> bool {
    if frame_in_flight {
        *latest_frame_dirty = true;
        return false;
    }

    true
}

fn finish_frame_present(frame_in_flight: &mut bool, latest_frame_dirty: &mut bool) -> bool {
    let should_emit_latest_image = *frame_in_flight && *latest_frame_dirty;

    *frame_in_flight = false;
    *latest_frame_dirty = false;

    should_emit_latest_image
}

fn clipboard_message_kind(message: &ClipboardMessage) -> &'static str {
    match message {
        ClipboardMessage::SendInitiateCopy(_) => "SendInitiateCopy",
        ClipboardMessage::SendFormatData(_) => "SendFormatData",
        ClipboardMessage::SendInitiatePaste(_) => "SendInitiatePaste",
        ClipboardMessage::SendLockClipboard { .. } => "SendLockClipboard",
        ClipboardMessage::SendUnlockClipboard { .. } => "SendUnlockClipboard",
        ClipboardMessage::SendFileContentsRequest(_) => "SendFileContentsRequest",
        ClipboardMessage::SendFileContentsResponse(_) => "SendFileContentsResponse",
        ClipboardMessage::Error(_) => "Error",
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
        // Build the pacing sleep future.  When no deadline is set the arm
        // resolves to a perpetually-pending future so it never fires spuriously.
        let pacing_sleep = async {
            match driver.frame_pacing_deadline {
                Some(deadline) => tokio_time::sleep_until(deadline).await,
                None => core::future::pending::<()>().await,
            }
        };

        let flow = tokio::select! {
            frame = reader.read_pdu() => {
                let (action, payload) = frame.map_err(|e| session::custom_err!("read frame", e))?;
                trace!(?action, frame_length = payload.len(), "Frame received");
                SessionDriverFlow::Outputs(driver.process_server_frame(action, &payload)?)
            }
            input_event = input_event_receiver.recv() => {
                let input_event = match input_event {
                    Some(event) => event,
                    None => {
                        // The GUI event loop dropped the input channel — this happens when the
                        // window is destroyed before a graceful-shutdown sequence completes
                        // (e.g., the process is killed or the event loop exits abnormally).
                        // Treat it as a hard disconnect rather than a protocol error.
                        debug!("Input channel closed (GUI stopped); treating as hard disconnect");
                        return Err(session::general_err!("GUI stopped unexpectedly"));
                    }
                };
                driver.handle_input_event(input_event)?
            }
            _ = pacing_sleep => {
                driver.frame_pacing_deadline = None;
                trace!("Frame pacing timer fired, emitting coalesced frame");
                SessionDriverFlow::EmitLatestImage
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
            SessionDriverFlow::EmitLatestImage => {
                driver.queue_latest_image_update(event_loop_proxy)?;
            }
            SessionDriverFlow::ReconnectWithNewSize { width, height } => {
                return Ok(RdpControlFlow::ReconnectWithNewSize { width, height });
            }
        }
    };

    Ok(RdpControlFlow::TerminatedGracefully(disconnect_reason))
}

/// Copy the RGBA frame from `image_data` into `buffer`.
///
/// When `dirty` is `Some(rect)` and the buffer already holds a full frame of
/// the correct size, only the rows `rect.top..=rect.bottom` are overwritten.
/// All other rows retain their previous values from the recycled buffer, so
/// the presentation backend always receives a complete, consistent frame.
///
/// When `dirty` is `None`, or when the buffer length does not match
/// `image_data` (first frame, resize, etc.), a full copy is performed.
///
/// Returns `true` if a partial (dirty-region-only) copy was performed, or
/// `false` if a full copy was performed.
///
/// # Errors
///
/// Returns an error if `image_data` length is not divisible by four.
fn copy_rgba_frame(
    image_data: &[u8],
    buffer: &mut Vec<u8>,
    width: u16,
    dirty: Option<&InclusiveRectangle>,
) -> SessionResult<bool> {
    if !image_data.len().is_multiple_of(4) {
        return Err(session::general_err!("decoded image length is not divisible by four"));
    }

    // Attempt a partial copy only when we have a valid dirty rect and the
    // buffer is already fully populated (recycled from the previous frame).
    if let Some(rect) = dirty
        && buffer.len() == image_data.len()
    {
        // Number of bytes per row (4 bytes per RGBA pixel × width).
        let stride = usize::from(width) * 4;

        // Guard against a malformed rect that extends beyond the image.
        let row_count = image_data.len() / stride.max(1);
        let top = usize::from(rect.top);
        let bottom = usize::from(rect.bottom).min(row_count.saturating_sub(1));

        if top <= bottom && stride > 0 {
            let src_start = top * stride;
            let src_end = (bottom + 1) * stride;
            buffer[src_start..src_end].copy_from_slice(&image_data[src_start..src_end]);
            return Ok(true);
        }
    }

    // Full copy: replaces buffer contents entirely.
    buffer.clear();
    buffer.extend_from_slice(image_data);

    Ok(false)
}

#[cfg(test)]
mod tests {
    use ironrdp::pdu::geometry::InclusiveRectangle;

    use super::{copy_rgba_frame, finish_frame_present, should_emit_latest_image, union_dirty_region};

    #[test]
    fn copy_rgba_frame_preserves_rgba_bytes() {
        // 2-pixel image, 1 row wide=2
        let image = [
            0x11, 0x22, 0x33, 0xff, //
            0x44, 0x55, 0x66, 0x77,
        ];
        let mut buffer = Vec::new();

        let partial = copy_rgba_frame(&image, &mut buffer, 2, None).expect("copy frame");

        assert!(!partial);
        assert_eq!(buffer, image);
    }

    #[test]
    fn copy_rgba_frame_reuses_existing_capacity() {
        let image = [
            0x01, 0x02, 0x03, 0xff, //
            0x04, 0x05, 0x06, 0xff,
        ];
        let mut buffer = Vec::with_capacity(8);
        let initial_capacity = buffer.capacity();

        let partial = copy_rgba_frame(&image, &mut buffer, 2, None).expect("copy frame");

        assert!(!partial);
        assert_eq!(buffer.len(), image.len());
        assert_eq!(buffer.capacity(), initial_capacity);
    }

    /// A 4×2 image (4 pixels wide, 2 rows).  We mark row 0 as dirty and
    /// verify only that row is updated while row 1 retains its prior value.
    #[test]
    fn copy_rgba_frame_partial_updates_only_dirty_rows() {
        // Row 0: four pixels (indices 0..=15), Row 1: four pixels (indices 16..=31)
        let old_row0 = [0x00u8; 16];
        let new_row0 = [0xAAu8; 16];
        let old_row1 = [0xFFu8; 16];
        let new_row1 = [0xFFu8; 16]; // same in source image

        let mut image = [0u8; 32];
        image[..16].copy_from_slice(&new_row0);
        image[16..].copy_from_slice(&new_row1);

        // Pre-populate the buffer with "stale" row 0 and sentinel row 1.
        let mut buffer = Vec::with_capacity(32);
        buffer.extend_from_slice(&old_row0);
        buffer.extend_from_slice(&old_row1);

        let dirty = InclusiveRectangle {
            left: 0,
            top: 0,
            right: 3,
            bottom: 0,
        };

        let partial = copy_rgba_frame(&image, &mut buffer, 4, Some(&dirty)).expect("partial copy");

        assert!(partial, "expected a partial copy");
        // Row 0 must be updated from the new image.
        assert_eq!(&buffer[..16], &new_row0);
        // Row 1 was not in the dirty rect so it keeps its old value.
        assert_eq!(&buffer[16..], &old_row1);
    }

    /// When the buffer is empty (first frame), a dirty rect must fall back to
    /// a full copy so the buffer is fully populated.
    #[test]
    fn copy_rgba_frame_partial_falls_back_when_buffer_empty() {
        let image = [0xBBu8; 16];
        let mut buffer = Vec::new(); // empty — no prior frame

        let dirty = InclusiveRectangle {
            left: 0,
            top: 0,
            right: 3,
            bottom: 0,
        };

        let partial = copy_rgba_frame(&image, &mut buffer, 4, Some(&dirty)).expect("copy with empty buffer");

        assert!(!partial, "must fall back to full copy when buffer is empty");
        assert_eq!(buffer, image.as_ref());
    }

    #[test]
    fn dirty_frame_is_re_emitted_only_after_present_ack() {
        let mut frame_in_flight = false;
        let mut latest_frame_dirty = false;

        assert!(should_emit_latest_image(frame_in_flight, &mut latest_frame_dirty));
        frame_in_flight = true;
        latest_frame_dirty = false;

        assert!(!should_emit_latest_image(frame_in_flight, &mut latest_frame_dirty));
        assert!(latest_frame_dirty);
        assert!(frame_in_flight);

        assert!(finish_frame_present(&mut frame_in_flight, &mut latest_frame_dirty));
        assert!(!frame_in_flight);
        assert!(!latest_frame_dirty);
    }

    #[test]
    fn union_dirty_region_starts_from_none() {
        let mut acc: Option<InclusiveRectangle> = None;
        let rect = InclusiveRectangle {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40,
        };
        union_dirty_region(&mut acc, rect.clone());
        assert_eq!(acc, Some(rect));
    }

    #[test]
    fn union_dirty_region_expands_bounding_box() {
        let mut acc = Some(InclusiveRectangle {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40,
        });
        union_dirty_region(
            &mut acc,
            InclusiveRectangle {
                left: 5,
                top: 25,
                right: 50,
                bottom: 35,
            },
        );
        assert_eq!(
            acc,
            Some(InclusiveRectangle {
                left: 5,
                top: 20,
                right: 50,
                bottom: 40,
            })
        );
    }
}
