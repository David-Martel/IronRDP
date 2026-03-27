//! Native windowing and rendering bridge for the IronRDP client.
//!
//! This module owns the `winit` application handler, presentation backend
//! selection, and translation from desktop-window input into [`RdpInputEvent`]
//! values consumed by the active session driver, including IME commit handling
//! for Unicode text. It also records lightweight surface-resize and software-
//! present timings through `tracing` so render-path changes can be measured
//! before deeper GPU work.
//!
//! [`RdpInputEvent`]: crate::rdp::RdpInputEvent

#![allow(clippy::print_stderr, clippy::print_stdout)] // allowed in this module only

use core::num::{NonZeroU16, NonZeroU32};
use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use proc_exit::Code;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{self, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::platform::scancode::PhysicalKeyExtScancode as _;
use winit::window::{CursorIcon, CustomCursor, Fullscreen, Window, WindowAttributes};

use crate::presentation::{PresentationBackend, SoftbufferBackend};
use crate::rdp::{RdpInputEvent, RdpOutputEvent};

const RESIZE_DEBOUNCE: Duration = Duration::from_millis(250);

/// PS/2 scancode for the Enter key (both main and numpad share this base code).
const SCANCODE_ENTER: u32 = 0x1C;

struct WindowState {
    window: Arc<Window>,
    presenter: Box<dyn PresentationBackend>,
}

impl WindowState {
    fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let presenter = SoftbufferBackend::new(Arc::clone(&window))?;

        Ok(Self {
            window,
            presenter: Box::new(presenter),
        })
    }
}

pub struct App {
    input_event_sender: mpsc::UnboundedSender<RdpInputEvent>,
    initial_size: PhysicalSize<u32>,
    server_name: String,
    window_state: Option<WindowState>,
    buffer: Vec<u8>,
    frame_pending_present: bool,
    redraw_requested: bool,
    buffer_size: (u16, u16),
    surface_size: (u16, u16),
    input_database: ironrdp::input::Database,
    ime_preedit_active: bool,
    last_size: Option<PhysicalSize<u32>>,
    resize_timeout: Option<Instant>,
    presented_frame_count: u64,
    surface_resize_count: u64,
    overwritten_frame_count: u64,
    pending_after_immediate_draw_count: u64,
    exit_code: Code,
    /// Exit code to use when the user manually closes the window.
    ///
    /// Set to an error code when the session fails so that a user-initiated close
    /// after reading the error title still propagates the correct exit status.
    pending_close_code: Option<Code>,
    /// Whether the window is currently in borderless fullscreen mode.
    is_fullscreen: bool,
    /// Tracks the Ctrl modifier state, updated from `ModifiersChanged` events.
    ctrl_pressed: bool,
    /// Tracks the Alt modifier state, updated from `ModifiersChanged` events.
    alt_pressed: bool,
}

impl App {
    pub fn new(
        input_event_sender: &mpsc::UnboundedSender<RdpInputEvent>,
        initial_size: PhysicalSize<u32>,
        server_name: String,
    ) -> anyhow::Result<Self> {
        let input_database = ironrdp::input::Database::new();
        Ok(Self {
            input_event_sender: input_event_sender.clone(),
            initial_size,
            server_name,
            window_state: None,
            buffer: Vec::new(),
            frame_pending_present: false,
            redraw_requested: false,
            buffer_size: (0, 0),
            surface_size: (0, 0),
            input_database,
            ime_preedit_active: false,
            last_size: None,
            resize_timeout: None,
            presented_frame_count: 0,
            surface_resize_count: 0,
            overwritten_frame_count: 0,
            pending_after_immediate_draw_count: 0,
            exit_code: Code::SUCCESS,
            pending_close_code: None,
            is_fullscreen: false,
            ctrl_pressed: false,
            alt_pressed: false,
        })
    }

    pub fn exit_code(&self) -> Code {
        self.exit_code
    }

    fn exit_with_code(&mut self, event_loop: &ActiveEventLoop, code: Code) {
        self.exit_code = code;
        event_loop.exit();
    }

    fn send_resize_event(&mut self) {
        let Some(size) = self.last_size.take() else {
            return;
        };
        let Some(window_state) = self.window_state.as_ref() else {
            return;
        };
        let window = &window_state.window;
        #[expect(clippy::as_conversions, reason = "casting f64 to u32")]
        let scale_factor = (window.scale_factor() * 100.0) as u32;

        let width = u16::try_from(size.width).expect("reasonable width");
        let height = u16::try_from(size.height).expect("reasonable height");

        let _ = self.input_event_sender.send(RdpInputEvent::Resize {
            width,
            height,
            scale_factor,
            // TODO: it should be possible to get the physical size here, however winit doesn't make it straightforward.
            // FreeRDP does it based on DPI reading grabbed via [`SDL_GetDisplayDPI`](https://wiki.libsdl.org/SDL2/SDL_GetDisplayDPI):
            // https://github.com/FreeRDP/FreeRDP/blob/ba8cf8cf2158018fb7abbedb51ab245f369be813/client/SDL/sdl_monitor.cpp#L250-L262
            // See also: https://github.com/rust-windowing/winit/issues/826
            physical_size: None,
        });
    }

    fn draw(&mut self) {
        self.redraw_requested = false;

        if self.buffer.is_empty() {
            return;
        }
        let Some(window_state) = self.window_state.as_mut() else {
            return;
        };
        let draw_started_at = Instant::now();
        let stats = match window_state
            .presenter
            .present_rgba(&self.buffer, self.buffer_size.0, self.buffer_size.1)
        {
            Ok(stats) => stats,
            Err(error) => {
                error!(%error, "Failed to present surface buffer");
                return;
            }
        };

        let frame_was_pending = self.frame_pending_present;
        self.presented_frame_count = self.presented_frame_count.saturating_add(1);
        self.frame_pending_present = false;
        if frame_was_pending {
            let _ = self.input_event_sender.send(RdpInputEvent::FramePresented);
        }
        if self.presented_frame_count == 1 {
            info!(
                width = self.buffer_size.0,
                height = self.buffer_size.1,
                "First frame presented to the window"
            );
            window_state
                .window
                .set_title(&format!("IronRDP \u{2014} {}", self.server_name));
        }
        trace!(
            frame_id = self.presented_frame_count,
            width = self.buffer_size.0,
            height = self.buffer_size.1,
            acquire_micros = stats.acquire_micros,
            convert_micros = stats.convert_micros,
            present_micros = stats.present_micros,
            backend_total_micros = stats.total_micros,
            total_micros = draw_started_at.elapsed().as_micros(),
            "Presented frame"
        );
    }

    fn queue_image_buffer(
        &mut self,
        buffer: Vec<u8>,
        width: NonZeroU16,
        height: NonZeroU16,
    ) -> Option<(Vec<u8>, bool)> {
        let recycled = if self.buffer.is_empty() {
            self.buffer = buffer;
            None
        } else {
            Some((core::mem::replace(&mut self.buffer, buffer), self.frame_pending_present))
        };

        self.buffer_size = (width.get(), height.get());
        self.frame_pending_present = true;

        recycled
    }
}

impl ApplicationHandler<RdpOutputEvent> for App {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(timeout) = self.resize_timeout {
            if let Some(timeout) = timeout.checked_duration_since(Instant::now()) {
                event_loop.set_control_flow(ControlFlow::wait_duration(timeout));
            } else {
                self.send_resize_event();
                self.resize_timeout = None;
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default()
            .with_title(format!("IronRDP \u{2014} Connecting to {}...", self.server_name))
            .with_inner_size(self.initial_size);
        match event_loop.create_window(window_attributes) {
            Ok(window) => {
                let window = Arc::new(window);
                match WindowState::new(window) {
                    Ok(window_state) => {
                        window_state.window.set_ime_allowed(true);
                        self.window_state = Some(window_state);
                    }
                    Err(error) => {
                        error!(%error, "Failed to create drawing surface");
                        self.exit_with_code(event_loop, proc_exit::sysexits::TEMP_FAIL);
                    }
                }
            }
            Err(error) => {
                error!(%error, "Failed to create window");
                self.exit_with_code(event_loop, proc_exit::sysexits::TEMP_FAIL);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: winit::window::WindowId, event: WindowEvent) {
        let Some(window_state) = self.window_state.as_mut() else {
            return;
        };
        let window = Arc::clone(&window_state.window);
        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::Resized(size) => {
                self.last_size = Some(size);
                self.resize_timeout = Some(Instant::now() + RESIZE_DEBOUNCE);
            }
            WindowEvent::CloseRequested => {
                if self.input_event_sender.send(RdpInputEvent::Close).is_err() {
                    // The session task has already exited (e.g. after a connection failure or
                    // normal termination).  Use the stored error code so the process exits with
                    // the status that was set when the failure was first reported.
                    let code = self.pending_close_code.unwrap_or(Code::FAILURE);
                    self.exit_with_code(event_loop, code);
                }
            }
            WindowEvent::DroppedFile(_) => {
                // TODO(#110): File upload
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // Intercept Ctrl+Alt+Enter before any other handling (including IME) to toggle
                // borderless fullscreen locally without forwarding the key combo to the RDP session.
                if event.state == event::ElementState::Pressed
                    && self.ctrl_pressed
                    && self.alt_pressed
                    && event.physical_key.to_scancode() == Some(SCANCODE_ENTER)
                {
                    if self.is_fullscreen {
                        window.set_fullscreen(None);
                        self.is_fullscreen = false;
                        debug!("Exited fullscreen mode");
                    } else {
                        window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                        self.is_fullscreen = true;
                        debug!("Entered fullscreen mode");
                    }
                    return;
                }

                if self.ime_preedit_active {
                    trace!("Ignoring raw keyboard input while IME preedit is active");
                    return;
                }

                if let Some(scancode) = event.physical_key.to_scancode() {
                    let scancode = match u16::try_from(scancode) {
                        Ok(scancode) => scancode,
                        Err(_) => {
                            warn!("Unsupported scancode: `{scancode:#X}`; ignored");
                            return;
                        }
                    };
                    let scancode = ironrdp::input::Scancode::from_u16(scancode);

                    let operation = match event.state {
                        event::ElementState::Pressed => ironrdp::input::Operation::KeyPressed(scancode),
                        event::ElementState::Released => ironrdp::input::Operation::KeyReleased(scancode),
                    };

                    let input_events = self.input_database.apply(core::iter::once(operation));

                    send_fast_path_events(&self.input_event_sender, input_events);
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.ime_preedit_active = false;
                let operations = unicode_text_operations(&text);
                if !operations.is_empty() {
                    let input_events = self.input_database.apply(operations);
                    send_fast_path_events(&self.input_event_sender, input_events);
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                self.ime_preedit_active = !text.is_empty();
            }
            WindowEvent::Ime(Ime::Disabled) => {
                self.ime_preedit_active = false;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if self.ime_preedit_active {
                    trace!("Ignoring modifier changes while IME preedit is active");
                    return;
                }

                // Track Ctrl and Alt state for the Ctrl+Alt+Enter fullscreen toggle.
                self.ctrl_pressed = modifiers.state().control_key();
                self.alt_pressed = modifiers.state().alt_key();

                const SHIFT_LEFT: ironrdp::input::Scancode = ironrdp::input::Scancode::from_u8(false, 0x2A);
                const CONTROL_LEFT: ironrdp::input::Scancode = ironrdp::input::Scancode::from_u8(false, 0x1D);
                const ALT_LEFT: ironrdp::input::Scancode = ironrdp::input::Scancode::from_u8(false, 0x38);
                const LOGO_LEFT: ironrdp::input::Scancode = ironrdp::input::Scancode::from_u8(true, 0x5B);

                let mut operations = smallvec::SmallVec::<[ironrdp::input::Operation; 4]>::new();

                let mut add_operation = |pressed: bool, scancode: ironrdp::input::Scancode| {
                    let operation = if pressed {
                        ironrdp::input::Operation::KeyPressed(scancode)
                    } else {
                        ironrdp::input::Operation::KeyReleased(scancode)
                    };
                    operations.push(operation);
                };

                // NOTE: https://docs.rs/winit/0.30.12/src/winit/keyboard.rs.html#1737-1744
                //
                // We can’t use state.lshift_state(), state.lcontrol_state(), etc, because on some platforms such as
                // Linux, the modifiers change is hidden.
                //
                // > The exact modifier key is not used to represent modifiers state in the
                // > first place due to a fact that modifiers state could be changed without any
                // > key being pressed and on some platforms like Wayland/X11 which key resulted
                // > in modifiers change is hidden, also, not that it really matters.
                add_operation(modifiers.state().shift_key(), SHIFT_LEFT);
                add_operation(modifiers.state().control_key(), CONTROL_LEFT);
                add_operation(modifiers.state().alt_key(), ALT_LEFT);
                add_operation(modifiers.state().super_key(), LOGO_LEFT);

                let input_events = self.input_database.apply(operations);

                send_fast_path_events(&self.input_event_sender, input_events);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let win_size = window.inner_size();
                #[expect(clippy::as_conversions, reason = "casting f64 to u16")]
                let x = (position.x / f64::from(win_size.width) * f64::from(self.buffer_size.0)) as u16;
                #[expect(clippy::as_conversions, reason = "casting f64 to u16")]
                let y = (position.y / f64::from(win_size.height) * f64::from(self.buffer_size.1)) as u16;
                let operation = ironrdp::input::Operation::MouseMove(ironrdp::input::MousePosition { x, y });

                let input_events = self.input_database.apply(core::iter::once(operation));

                send_fast_path_events(&self.input_event_sender, input_events);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let mut operations = smallvec::SmallVec::<[ironrdp::input::Operation; 2]>::new();

                match delta {
                    event::MouseScrollDelta::LineDelta(delta_x, delta_y) => {
                        if delta_x.abs() > 0.001 {
                            operations.push(ironrdp::input::Operation::WheelRotations(
                                ironrdp::input::WheelRotations {
                                    is_vertical: false,
                                    #[expect(clippy::as_conversions, reason = "casting f32 to i16")]
                                    rotation_units: (delta_x * 100.) as i16,
                                },
                            ));
                        }

                        if delta_y.abs() > 0.001 {
                            operations.push(ironrdp::input::Operation::WheelRotations(
                                ironrdp::input::WheelRotations {
                                    is_vertical: true,
                                    #[expect(clippy::as_conversions, reason = "casting f32 to i16")]
                                    rotation_units: (delta_y * 100.) as i16,
                                },
                            ));
                        }
                    }
                    event::MouseScrollDelta::PixelDelta(delta) => {
                        if delta.x.abs() > 0.001 {
                            operations.push(ironrdp::input::Operation::WheelRotations(
                                ironrdp::input::WheelRotations {
                                    is_vertical: false,
                                    #[expect(clippy::as_conversions, reason = "casting f64 to i16")]
                                    rotation_units: delta.x as i16,
                                },
                            ));
                        }

                        if delta.y.abs() > 0.001 {
                            operations.push(ironrdp::input::Operation::WheelRotations(
                                ironrdp::input::WheelRotations {
                                    is_vertical: true,
                                    #[expect(clippy::as_conversions, reason = "casting f64 to i16")]
                                    rotation_units: delta.y as i16,
                                },
                            ));
                        }
                    }
                };

                let input_events = self.input_database.apply(operations);

                send_fast_path_events(&self.input_event_sender, input_events);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let mouse_button = match button {
                    event::MouseButton::Left => ironrdp::input::MouseButton::Left,
                    event::MouseButton::Right => ironrdp::input::MouseButton::Right,
                    event::MouseButton::Middle => ironrdp::input::MouseButton::Middle,
                    event::MouseButton::Back => ironrdp::input::MouseButton::X1,
                    event::MouseButton::Forward => ironrdp::input::MouseButton::X2,
                    event::MouseButton::Other(native_button) => {
                        if let Some(button) = ironrdp::input::MouseButton::from_native_button(native_button) {
                            button
                        } else {
                            return;
                        }
                    }
                };

                let operation = match state {
                    event::ElementState::Pressed => ironrdp::input::Operation::MouseButtonPressed(mouse_button),
                    event::ElementState::Released => ironrdp::input::Operation::MouseButtonReleased(mouse_button),
                };

                let input_events = self.input_database.apply(core::iter::once(operation));

                send_fast_path_events(&self.input_event_sender, input_events);
            }
            WindowEvent::RedrawRequested => {
                self.draw();
            }
            WindowEvent::ActivationTokenDone { .. }
            | WindowEvent::Moved(_)
            | WindowEvent::Destroyed
            | WindowEvent::HoveredFile(_)
            | WindowEvent::HoveredFileCancelled
            | WindowEvent::Focused(_)
            | WindowEvent::Ime(Ime::Enabled)
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::PinchGesture { .. }
            | WindowEvent::PanGesture { .. }
            | WindowEvent::DoubleTapGesture { .. }
            | WindowEvent::RotationGesture { .. }
            | WindowEvent::TouchpadPressure { .. }
            | WindowEvent::AxisMotion { .. }
            | WindowEvent::Touch(_)
            | WindowEvent::ThemeChanged(_)
            | WindowEvent::Occluded(_) => {
                // ignore
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.last_size = Some(window.inner_size());
                self.resize_timeout = Some(Instant::now() + RESIZE_DEBOUNCE);
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: RdpOutputEvent) {
        let Some(window) = self
            .window_state
            .as_ref()
            .map(|window_state| Arc::clone(&window_state.window))
        else {
            return;
        };
        match event {
            RdpOutputEvent::Image { buffer, width, height } => {
                trace!(width = ?width, height = ?height, "Received image with size");
                trace!(window_physical_size = ?window.inner_size(), "Drawing image to the window with size");
                let previous_buffer_size = self.buffer_size;
                let frame_was_pending = self.frame_pending_present;
                if let Some((recycled_buffer, overwritten_unpresented)) = self.queue_image_buffer(buffer, width, height)
                {
                    if overwritten_unpresented {
                        self.overwritten_frame_count = self.overwritten_frame_count.saturating_add(1);
                        trace!(
                            overwritten_frame_count = self.overwritten_frame_count,
                            buffered_width = previous_buffer_size.0,
                            buffered_height = previous_buffer_size.1,
                            "Overwriting unpresented frame buffer"
                        );
                    } else {
                        trace!(
                            buffered_width = previous_buffer_size.0,
                            buffered_height = previous_buffer_size.1,
                            "Replacing already presented frame buffer"
                        );
                    }
                    let _ = self
                        .input_event_sender
                        .send(RdpInputEvent::RecycleFrameBuffer(recycled_buffer));
                }
                if self.surface_size != self.buffer_size {
                    let Some(window_state) = self.window_state.as_mut() else {
                        return;
                    };
                    if let Err(error) = window_state
                        .presenter
                        .resize(NonZeroU32::from(width), NonZeroU32::from(height))
                    {
                        error!(%error, "Failed to resize drawing surface");
                        self.exit_with_code(event_loop, proc_exit::sysexits::TEMP_FAIL);
                        return;
                    }
                    self.surface_size = self.buffer_size;
                    self.surface_resize_count = self.surface_resize_count.saturating_add(1);
                    debug!(
                        surface_resize_count = self.surface_resize_count,
                        width = self.surface_size.0,
                        height = self.surface_size.1,
                        "Resized presentation surface"
                    );
                }

                self.draw();
                if self.frame_pending_present && !self.redraw_requested {
                    self.pending_after_immediate_draw_count =
                        self.pending_after_immediate_draw_count.saturating_add(1);
                    trace!(
                        pending_after_immediate_draw_count = self.pending_after_immediate_draw_count,
                        frame_was_already_pending = frame_was_pending,
                        "Image remained pending after an immediate draw attempt"
                    );
                    window.request_redraw();
                    self.redraw_requested = true;
                }
            }
            RdpOutputEvent::ConnectionFailure(error) => {
                let msg = error.report().to_string();
                error!(?error, "Connection failed");
                eprintln!("Connection error: {msg}");
                // Keep the window open so the user can read the error message.
                // Truncate to 100 characters so the title bar remains readable.
                let truncated = truncate_for_title(&msg, 100);
                window.set_title(&format!("IronRDP \u{2014} Connection failed: {truncated}"));
                // Store the exit code so that when the user manually closes the window
                // the process exits with the correct status.
                self.pending_close_code = Some(proc_exit::sysexits::PROTOCOL_ERR);
            }
            RdpOutputEvent::Terminated(result) => {
                match result {
                    Ok(reason) => {
                        let msg = capitalize_first(&format!("terminated gracefully: {reason}"));
                        info!(%reason, "Session ended: graceful disconnect");
                        println!("{msg}");
                        window.set_title(&format!("IronRDP \u{2014} {msg}"));
                        self.exit_with_code(event_loop, proc_exit::sysexits::OK);
                    }
                    Err(ref error) if error.to_string().contains("GUI stopped unexpectedly") => {
                        // The input channel was dropped because the event loop exited before the
                        // RDP task.  This is an expected teardown race; do not show an error UI.
                        debug!("Session task observed GUI shutdown; exiting cleanly");
                        self.exit_with_code(event_loop, proc_exit::sysexits::OK);
                    }
                    Err(error) => {
                        let msg = error.report().to_string();
                        error!(?error, "Session ended: transport or protocol error");
                        eprintln!("Active session error: {msg}");
                        // Keep the window open so the user can read the error message.
                        let truncated = truncate_for_title(&msg, 100);
                        window.set_title(&format!("IronRDP \u{2014} Session error: {truncated}"));
                        // Store the exit code for when the user closes the window.
                        self.pending_close_code = Some(proc_exit::sysexits::PROTOCOL_ERR);
                    }
                }
            }
            RdpOutputEvent::PointerHidden => {
                window.set_cursor_visible(false);
            }
            RdpOutputEvent::PointerDefault => {
                window.set_cursor(CursorIcon::default());
                window.set_cursor_visible(true);
            }
            RdpOutputEvent::PointerPosition { x, y } => {
                // `x` and `y` are RDP guest desktop coordinates (buffer space). To correctly
                // position the cursor on a HiDPI display we must map them back to physical
                // window pixels — the exact inverse of the `CursorMoved` handler which maps
                // physical pixels → buffer coordinates.
                //
                //   x_physical = x / buffer_width  * win_physical_width
                //   y_physical = y / buffer_height * win_physical_height
                //
                // Using `PhysicalPosition` avoids a second DPI scale application that
                // `LogicalPosition` would introduce, which misplaced the cursor at scales
                // other than 100 %.
                let win_size = window.inner_size();
                let x_physical = f64::from(x) / f64::from(self.buffer_size.0) * f64::from(win_size.width);
                let y_physical = f64::from(y) / f64::from(self.buffer_size.1) * f64::from(win_size.height);
                if let Err(error) = window.set_cursor_position(PhysicalPosition::new(x_physical, y_physical)) {
                    error!(?error, "Failed to set cursor position");
                }
            }
            RdpOutputEvent::PointerBitmap(pointer) => {
                debug!(width = ?pointer.width, height = ?pointer.height, "Received pointer bitmap");
                let pointer = match Arc::try_unwrap(pointer) {
                    Ok(pointer) => pointer,
                    Err(pointer) => ironrdp::graphics::pointer::DecodedPointer {
                        width: pointer.width,
                        height: pointer.height,
                        hotspot_x: pointer.hotspot_x,
                        hotspot_y: pointer.hotspot_y,
                        bitmap_data: pointer.bitmap_data.clone(),
                    },
                };
                match CustomCursor::from_rgba(
                    pointer.bitmap_data,
                    pointer.width,
                    pointer.height,
                    pointer.hotspot_x,
                    pointer.hotspot_y,
                ) {
                    Ok(cursor) => window.set_cursor(event_loop.create_custom_cursor(cursor)),
                    Err(error) => error!(?error, "Failed to set cursor bitmap"),
                }
                window.set_cursor_visible(true);
            }
        }
    }
}

fn send_fast_path_events(
    input_event_sender: &mpsc::UnboundedSender<RdpInputEvent>,
    input_events: smallvec::SmallVec<[ironrdp::pdu::input::fast_path::FastPathInputEvent; 2]>,
) {
    if !input_events.is_empty() {
        let _ = input_event_sender.send(RdpInputEvent::FastPath(input_events));
    }
}

/// Capitalizes the first Unicode scalar value of `s`.
///
/// Returns `s` unchanged when it is empty or its first character has no uppercase form.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Truncates `s` to at most `max_chars` Unicode scalar values.
///
/// Truncation happens on a character boundary so the result is always valid UTF-8.
/// If the string is truncated a horizontal ellipsis (`…`) is appended to indicate
/// that the message was cut short.
fn truncate_for_title(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        truncated + "\u{2026}" // U+2026 HORIZONTAL ELLIPSIS
    } else {
        truncated
    }
}

fn unicode_text_operations(text: &str) -> smallvec::SmallVec<[ironrdp::input::Operation; 8]> {
    let mut operations = smallvec::SmallVec::new();

    for character in text.chars().filter(|character| !character.is_control()) {
        operations.push(ironrdp::input::Operation::UnicodeKeyPressed(character));
        operations.push(ironrdp::input::Operation::UnicodeKeyReleased(character));
    }

    operations
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU16;

    use ironrdp::input::Operation;
    use tokio::sync::mpsc;
    use winit::dpi::PhysicalSize;

    use super::{App, capitalize_first, truncate_for_title, unicode_text_operations};

    #[test]
    fn capitalize_first_empty_string() {
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn capitalize_first_already_capitalized() {
        assert_eq!(capitalize_first("Hello world"), "Hello world");
    }

    #[test]
    fn capitalize_first_lowercase_ascii() {
        assert_eq!(capitalize_first("hello world"), "Hello world");
    }

    #[test]
    fn capitalize_first_multibyte_char() {
        // 'é' → 'É'
        assert_eq!(capitalize_first("été"), "Été");
    }

    #[test]
    fn truncate_for_title_short_string_unchanged() {
        assert_eq!(truncate_for_title("hello", 10), "hello");
    }

    #[test]
    fn truncate_for_title_exact_length_unchanged() {
        assert_eq!(truncate_for_title("hello", 5), "hello");
    }

    #[test]
    fn truncate_for_title_long_string_truncated_with_ellipsis() {
        let result = truncate_for_title("abcdefghij", 5);
        assert_eq!(result, "abcde\u{2026}");
    }

    #[test]
    fn truncate_for_title_multibyte_chars_respected() {
        // Each '中' is 3 bytes but 1 char; limit=2 should give "中中…"
        let s = "中中中中";
        let result = truncate_for_title(s, 2);
        assert_eq!(result, "中中\u{2026}");
    }

    #[test]
    fn unicode_text_operations_emit_press_and_release_pairs() {
        let operations = unicode_text_operations("A\u{1f642}\n");

        assert_eq!(operations.len(), 4);
        assert!(matches!(operations[0], Operation::UnicodeKeyPressed('A')));
        assert!(matches!(operations[1], Operation::UnicodeKeyReleased('A')));
        assert!(matches!(operations[2], Operation::UnicodeKeyPressed('\u{1f642}')));
        assert!(matches!(operations[3], Operation::UnicodeKeyReleased('\u{1f642}')));
    }

    #[test]
    fn unicode_text_operations_ignore_control_characters() {
        let operations = unicode_text_operations("\r\n\t");

        assert!(operations.is_empty());
    }

    #[test]
    fn queue_image_buffer_only_counts_pending_frames_as_overwritten() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let mut app = App::new(&sender, PhysicalSize::new(640, 480), "test-server".to_owned()).expect("app");
        let width = NonZeroU16::new(640).expect("non-zero width");
        let height = NonZeroU16::new(480).expect("non-zero height");

        let recycled = app.queue_image_buffer(vec![0; 16], width, height);
        assert!(recycled.is_none());
        assert!(app.frame_pending_present);

        let recycled = app
            .queue_image_buffer(vec![1; 16], width, height)
            .expect("existing frame should be recycled");
        assert!(recycled.1, "replacing a pending frame should be marked as overwritten");
        assert!(app.frame_pending_present);

        app.frame_pending_present = false;

        let recycled = app
            .queue_image_buffer(vec![2; 16], width, height)
            .expect("existing frame should be recycled");
        assert!(
            !recycled.1,
            "replacing an already presented frame should not be marked as overwritten"
        );
        assert!(app.frame_pending_present);
    }
}
