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

use core::num::NonZeroU32;
use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use proc_exit::Code;
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, PhysicalSize};
use winit::event::{self, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::platform::scancode::PhysicalKeyExtScancode as _;
use winit::window::{CursorIcon, CustomCursor, Window, WindowAttributes};

use crate::presentation::{PresentationBackend, SoftbufferBackend};
use crate::rdp::{RdpInputEvent, RdpOutputEvent};

const RESIZE_DEBOUNCE: Duration = Duration::from_millis(250);

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
    window_state: Option<WindowState>,
    buffer: Vec<u8>,
    buffer_size: (u16, u16),
    surface_size: (u16, u16),
    input_database: ironrdp::input::Database,
    ime_preedit_active: bool,
    last_size: Option<PhysicalSize<u32>>,
    resize_timeout: Option<Instant>,
    presented_frame_count: u64,
    surface_resize_count: u64,
    exit_code: Code,
}

impl App {
    pub fn new(
        input_event_sender: &mpsc::UnboundedSender<RdpInputEvent>,
        initial_size: PhysicalSize<u32>,
    ) -> anyhow::Result<Self> {
        let input_database = ironrdp::input::Database::new();
        Ok(Self {
            input_event_sender: input_event_sender.clone(),
            initial_size,
            window_state: None,
            buffer: Vec::new(),
            buffer_size: (0, 0),
            surface_size: (0, 0),
            input_database,
            ime_preedit_active: false,
            last_size: None,
            resize_timeout: None,
            presented_frame_count: 0,
            surface_resize_count: 0,
            exit_code: Code::SUCCESS,
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

        self.presented_frame_count = self.presented_frame_count.saturating_add(1);
        trace!(
            frame_id = self.presented_frame_count,
            width = self.buffer_size.0,
            height = self.buffer_size.1,
            convert_micros = stats.convert_micros,
            present_micros = stats.present_micros,
            backend_total_micros = stats.total_micros,
            total_micros = draw_started_at.elapsed().as_micros(),
            "Presented frame"
        );
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
            .with_title("IronRDP")
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
                    error!("Failed to send graceful shutdown event, closing the window");
                    self.exit_with_code(event_loop, Code::FAILURE);
                }
            }
            WindowEvent::DroppedFile(_) => {
                // TODO(#110): File upload
            }
            WindowEvent::KeyboardInput { event, .. } => {
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
        let Some(window_state) = self.window_state.as_mut() else {
            return;
        };
        let window = Arc::clone(&window_state.window);
        match event {
            RdpOutputEvent::Image { buffer, width, height } => {
                trace!(width = ?width, height = ?height, "Received image with size");
                trace!(window_physical_size = ?window.inner_size(), "Drawing image to the window with size");
                if !self.buffer.is_empty() {
                    let recycled_buffer = core::mem::take(&mut self.buffer);
                    let _ = self
                        .input_event_sender
                        .send(RdpInputEvent::RecycleFrameBuffer(recycled_buffer));
                }
                self.buffer_size = (width.get(), height.get());
                self.buffer = buffer;
                if self.surface_size != self.buffer_size {
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

                window.request_redraw();
            }
            RdpOutputEvent::ConnectionFailure(error) => {
                error!(?error);
                eprintln!("Connection error: {}", error.report());
                self.exit_with_code(event_loop, proc_exit::sysexits::PROTOCOL_ERR);
            }
            RdpOutputEvent::Terminated(result) => {
                let exit_code = match result {
                    Ok(reason) => {
                        println!("Terminated gracefully: {reason}");
                        proc_exit::sysexits::OK
                    }
                    Err(error) => {
                        error!(?error);
                        eprintln!("Active session error: {}", error.report());
                        proc_exit::sysexits::PROTOCOL_ERR
                    }
                };
                self.exit_with_code(event_loop, exit_code);
            }
            RdpOutputEvent::PointerHidden => {
                window.set_cursor_visible(false);
            }
            RdpOutputEvent::PointerDefault => {
                window.set_cursor(CursorIcon::default());
                window.set_cursor_visible(true);
            }
            RdpOutputEvent::PointerPosition { x, y } => {
                if let Err(error) = window.set_cursor_position(LogicalPosition::new(x, y)) {
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
    use ironrdp::input::Operation;

    use super::unicode_text_operations;

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
}
