//! Windowed runner (winit 0.30). Handles the mobile lifecycle: the surface is
//! dropped on `suspended` and recreated on `resumed`, the GPU device survives.
#[cfg(target_os = "android")]
use super::AndroidApp;
use super::presentation::{self, PresentationBinding};
use super::{Code, Gpu, Input, Key, Mouse, PlatformApp, PlatformError};
use glam::Vec2;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

struct Surf {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

struct Runner<A: PlatformApp> {
    app: A,
    title: String,
    size: (u32, u32),
    instance: wgpu::Instance,
    gpu: Option<Gpu>,
    surf: Option<Surf>,
    initted: bool,
    input: Input,
    last: Instant,
    frames: u64,
    /// First fatal error raised inside a winit callback; `run_on` returns it once the loop exits.
    error: Option<PlatformError>,
}

/// Runs `app` in the window of a NativeActivity. The event loop is bound to `android`, the
/// activity handed to `android_main`; otherwise it behaves like `run`.
#[cfg(target_os = "android")]
pub fn run_android<A: PlatformApp>(
    android: AndroidApp,
    title: &str,
    size: (u32, u32),
    app: A,
) -> Result<(), PlatformError> {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    let event_loop = EventLoop::builder().with_android_app(android).build()?;
    run_on(event_loop, title, size, app)
}

/// Opens a window of `size` physical pixels titled `title` and runs `app` until it quits.
pub fn run<A: PlatformApp>(title: &str, size: (u32, u32), app: A) -> Result<(), PlatformError> {
    run_on(EventLoop::new()?, title, size, app)
}

/// Same as `run` with a caller-built event loop (Android needs one bound to its activity).
pub fn run_on<A: PlatformApp>(
    event_loop: EventLoop<()>,
    title: &str,
    size: (u32, u32),
    app: A,
) -> Result<(), PlatformError> {
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut r = Runner {
        app,
        title: title.to_string(),
        size,
        instance: wgpu::Instance::default(),
        gpu: None,
        surf: None,
        initted: false,
        input: Input::default(),
        last: Instant::now(),
        frames: 0,
        error: None,
    };
    event_loop.run_app(&mut r)?;
    match r.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn key_of(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::ArrowLeft | KeyCode::KeyA => Key::Left,
        KeyCode::ArrowRight | KeyCode::KeyD => Key::Right,
        KeyCode::ArrowUp | KeyCode::KeyW => Key::Up,
        KeyCode::ArrowDown | KeyCode::KeyS => Key::Down,
        KeyCode::Space => Key::Fire,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Boost,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Brake,
        KeyCode::KeyQ => Key::RollLeft,
        KeyCode::KeyE => Key::RollRight,
        KeyCode::KeyR => Key::Missile,
        KeyCode::KeyO => Key::Orbit,
        KeyCode::KeyI => Key::Stats,
        KeyCode::Escape => Key::Quit,
        _ => return None,
    })
}

fn code_of(c: KeyCode) -> Option<Code> {
    use KeyCode as K;
    Some(match c {
        K::KeyA => Code::A,
        K::KeyB => Code::B,
        K::KeyC => Code::C,
        K::KeyD => Code::D,
        K::KeyE => Code::E,
        K::KeyF => Code::F,
        K::KeyG => Code::G,
        K::KeyH => Code::H,
        K::KeyI => Code::I,
        K::KeyJ => Code::J,
        K::KeyK => Code::K,
        K::KeyL => Code::L,
        K::KeyM => Code::M,
        K::KeyN => Code::N,
        K::KeyO => Code::O,
        K::KeyP => Code::P,
        K::KeyQ => Code::Q,
        K::KeyR => Code::R,
        K::KeyS => Code::S,
        K::KeyT => Code::T,
        K::KeyU => Code::U,
        K::KeyV => Code::V,
        K::KeyW => Code::W,
        K::KeyX => Code::X,
        K::KeyY => Code::Y,
        K::KeyZ => Code::Z,
        K::Digit0 => Code::D0,
        K::Digit1 => Code::D1,
        K::Digit2 => Code::D2,
        K::Digit3 => Code::D3,
        K::Digit4 => Code::D4,
        K::Digit5 => Code::D5,
        K::Digit6 => Code::D6,
        K::Digit7 => Code::D7,
        K::Digit8 => Code::D8,
        K::Digit9 => Code::D9,
        K::Space => Code::Space,
        K::ShiftLeft | K::ShiftRight => Code::Shift,
        K::ControlLeft | K::ControlRight => Code::Ctrl,
        K::AltLeft | K::AltRight => Code::Alt,
        K::Tab => Code::Tab,
        K::Escape => Code::Esc,
        K::Enter | K::NumpadEnter => Code::Enter,
        K::Backspace => Code::Backspace,
        K::ArrowUp => Code::Up,
        K::ArrowDown => Code::Down,
        K::ArrowLeft => Code::Left,
        K::ArrowRight => Code::Right,
        K::F1 => Code::F1,
        K::F2 => Code::F2,
        K::F3 => Code::F3,
        K::F4 => Code::F4,
        K::F5 => Code::F5,
        _ => return None,
    })
}

/// winit reports positions and wheel deltas as f64. Rounding to f32 is intended: these are
/// screen-space values, finite and far inside f32 range, and f32 precision is ample for input.
#[allow(clippy::cast_possible_truncation)]
fn f32_of(v: f64) -> f32 {
    v as f32
}

impl<A: PlatformApp> Runner<A> {
    /// Apply the game's cursor-grab request. Locked is preferred (true relative motion);
    /// some platforms only support Confined.
    fn sync_grab(&mut self) {
        let Some(s) = self.surf.as_ref() else { return };
        if self.input.want_grab == self.input.grabbed {
            return;
        }
        if self.input.want_grab {
            let ok = s
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| s.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            s.window.set_cursor_visible(false);
            self.input.grabbed = ok;
            if !ok {
                // Still hide the cursor and pretend: raw motion keeps working.
                self.input.grabbed = true;
            }
        } else {
            let _ = s.window.set_cursor_grab(CursorGrabMode::None);
            s.window.set_cursor_visible(true);
            self.input.grabbed = false;
        }
    }

    fn configure(&mut self, w: u32, h: u32) {
        if let (Some(s), Some(g)) = (self.surf.as_mut(), self.gpu.as_ref()) {
            s.config.width = w.max(1);
            s.config.height = h.max(1);
            s.surface.configure(&g.device, &s.config);
        }
    }

    /// Records a fatal error and asks the loop to stop; `run_on` returns it.
    fn fail(&mut self, el: &ActiveEventLoop, e: PlatformError) {
        self.error = Some(e);
        el.exit();
    }

    /// Creates the window, surface and (first time only) the device, then configures the surface.
    fn try_resume(&mut self, el: &ActiveEventLoop) -> Result<(), PlatformError> {
        let attrs = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::PhysicalSize::new(self.size.0, self.size.1));
        let window = Arc::new(el.create_window(attrs)?);
        let surface = self.instance.create_surface(window.clone())?;
        let gpu = match self.gpu.as_ref() {
            Some(g) => g.clone(),
            None => {
                let g = Gpu::new(self.instance.clone(), Some(&surface))?;
                self.gpu = Some(g.clone());
                g
            }
        };
        let sz = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or(PlatformError::NoSurfaceFormat)?;
        let mut config = surface
            .get_default_config(&gpu.adapter, sz.width.max(1), sz.height.max(1))
            .ok_or(PlatformError::SurfaceUnsupported)?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&gpu.device, &config);
        let binding = presentation::bind_after_swapchain_configure(&window);
        self.deliver_presentation_binding(binding);
        if !self.initted {
            println!("gpu: {}", gpu.info());
            self.app.init(&gpu, format, (config.width, config.height));
            self.initted = true;
        } else {
            self.app.resume(&gpu, format, (config.width, config.height));
        }
        self.input.screen = Vec2::new(config.width as f32, config.height as f32);
        self.surf = Some(Surf {
            window,
            surface,
            config,
        });
        self.last = Instant::now();
        Ok(())
    }

    fn deliver_presentation_binding(&mut self, binding: PresentationBinding) {
        self.app.on_presentation_bound(binding);
    }
}

impl<A: PlatformApp> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if let Err(e) = self.try_resume(el) {
            self.fail(el, e);
        }
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        // Drop game renderer resources while the OS has revoked its presentation surface.
        self.app.suspend();
        self.surf = None;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                self.configure(s.width, s.height);
                self.input.screen = Vec2::new(s.width as f32, s.height as f32);
                self.app.resize((s.width.max(1), s.height.max(1)));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(c) = event.physical_key {
                    let down = event.state == ElementState::Pressed;
                    if let Some(code) = code_of(c) {
                        self.input.set_code(code, down);
                    }
                    if let Some(k) = key_of(c) {
                        self.input.set_key(k, down);
                        if k == Key::Quit && down && self.input.esc_quits {
                            el.exit();
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => Some(Mouse::Left),
                    MouseButton::Right => Some(Mouse::Right),
                    MouseButton::Middle => Some(Mouse::Middle),
                    _ => None,
                };
                if let Some(b) = b {
                    self.input.set_button(b, state == ElementState::Pressed);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.mouse_pos = Vec2::new(f32_of(position.x), f32_of(position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.input.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => f32_of(p.y) / 40.0,
                };
            }
            WindowEvent::Focused(false) => {
                self.input.release_all();
                self.input.grabbed = false;
                if let Some(s) = self.surf.as_ref() {
                    let _ = s.window.set_cursor_grab(CursorGrabMode::None);
                    s.window.set_cursor_visible(true);
                }
            }
            WindowEvent::Touch(t) => {
                let p = Vec2::new(f32_of(t.location.x), f32_of(t.location.y));
                match t.phase {
                    TouchPhase::Started => self.input.touch_start(t.id, p),
                    TouchPhase::Moved => self.input.touch_move(t.id, p),
                    TouchPhase::Ended | TouchPhase::Cancelled => self.input.touch_end(t.id),
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, ev: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = ev {
            self.input.mouse_delta += Vec2::new(f32_of(delta.0), f32_of(delta.1));
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.input.quit {
            el.exit();
        }
        self.sync_grab();
        if let Some(s) = &self.surf {
            s.window.request_redraw();
        }
    }
}

impl<A: PlatformApp> Runner<A> {
    fn redraw(&mut self) {
        let Some(s) = self.surf.as_ref() else { return };
        let Some(gpu) = self.gpu.as_ref() else { return };
        let frame = match s.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let (w, h) = (s.config.width, s.config.height);
                self.configure(w, h);
                return;
            }
            _ => return, // timeout / occluded: skip this frame
        };
        self.frames += 1;
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.app.frame(&view, &mut self.input, dt);
        self.input.end_frame();
        gpu.queue.present(frame);
    }
}
