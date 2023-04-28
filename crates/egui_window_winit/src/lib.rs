use egui::{DroppedFile, Event, Key, Modifiers, Rect};
use egui_backend::egui::RawInput;
use egui_backend::*;
pub use winit;
use winit::dpi::PhysicalSize;
use winit::{event::MouseButton, window::WindowBuilder, *};
use winit::{
    event::{ModifiersState, VirtualKeyCode},
    event_loop::{ControlFlow, EventLoop},
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use winit::platform::web::{WindowBuilderExtWebSys};

/// config that you provide to winit backend
#[derive(Debug)]
pub struct WinitConfig {
    #[cfg(target_os = "android")]
    pub android_app: winit::platform::android::activity::AndroidApp,
    /// window title
    pub title: String,
    /// on web: winit will try to get the canvas element with this id attribute and use it as the window's context
    /// for now, it must not be empty. we can later provide options like creating a canvas ourselves and adding it to dom
    /// default value is : `egui_canvas`
    /// so, make sure there's a canvas element in html body with this id
    pub dom_element_id: Option<String>,
}
impl Default for WinitConfig {
    fn default() -> Self {
        Self {
            title: "egui winit window".to_string(),
            dom_element_id: Some("egui_canvas".to_string()),
            #[cfg(target_os = "android")]
            android_app: unimplemented!(
                "winit requires android 'app' struct from android_main function"
            ),
        }
    }
}
/// This is the winit WindowBackend for egui
pub struct WinitBackend {
    /// we want to take out the event loop when we call the  `WindowBackend::run_event_loop` fn
    /// so, this will always be `None` once we start the event loop
    pub event_loop: Option<EventLoop<()>>,
    /// the winit window. on android, this might be None when suspended. and recreated when resumed.
    /// on other platforms, we just create the window before entering event loop.
    pub window: Option<winit::window::Window>,
    /// current modifiers state
    pub modifiers: egui::Modifiers,
    pub pointer_touch_id: Option<u64>,
    /// frame buffer size in physical pixels
    pub framebuffer_size: [u32; 2],
    /// scale
    pub scale: f32,
    /// cusor position in logical pixels
    pub cursor_pos_logical: [f32; 2],
    /// input for egui's begin_frame
    pub raw_input: RawInput,
    /// all current frame's events will be stored in this vec
    pub frame_events: Vec<winit::event::Event<'static, ()>>,
    /// should be true if there's been a resize event
    /// should be set to false once the renderer takes the latest size during `GfxBackend::prepare_frame`
    pub latest_resize_event: bool,
    /// ???
    pub should_close: bool,
    pub backend_config: BackendConfig,
    pub window_builder: WindowBuilder,
    pub winit_config: WinitConfig,
    #[cfg(target_arch = "wasm32")]
    last_canvas_parent_size: (i32, i32),
}

impl WindowBackend for WinitBackend {
    type Configuration = WinitConfig;
    type WindowType = winit::window::Window;

    fn new(
        winit_config: Self::Configuration,
        backend_config: BackendConfig,
        _width: u32,
        _height: u32,
    ) -> Self {
        let mut event_loop_builder = winit::event_loop::EventLoopBuilder::with_user_event();

        #[cfg(target_os = "android")]
        use winit::platform::android::EventLoopBuilderExtAndroid;
        #[cfg(target_os = "android")]
        let event_loop = event_loop.with_android_app(winit_config.android_app);

        let event_loop = event_loop_builder.build();
        tracing::info!("this is loggging");

        #[allow(unused_mut)]
        let mut window_builder =
            WindowBuilder::new()
                //.with_inner_size(PhysicalSize::new(width, height))
                .with_resizable(true)
                .with_title(&winit_config.title);

        #[cfg(target_arch = "wasm32")]
        let window = {
            let document = web_sys::window()
                .expect("failed ot get websys window")
                .document()
                .expect("failed to get websys doc");
            tracing::info!("this is web loggging");
            let canvas = winit_config.dom_element_id.as_ref().map(|canvas_id| {
                    document
                        .get_element_by_id(&canvas_id)
                        .expect("winit_config doesn't contain canvas and DOM doesn't have a canvas element either")
                        .dyn_into::<web_sys::HtmlCanvasElement>().expect("failed to get canvas converted into html canvas element")
                });
            window_builder = window_builder.with_canvas(canvas);
            // create winit window
            let window = window_builder
                .clone()
                .build(&event_loop)
                .expect("failed to create winit window");

            Some(window)
        };

        tracing::info!("this is not web");
        #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
        let window = Some(
            window_builder
                .clone()
                .build(&event_loop)
                .expect("failed ot create winit window"),
        );

        #[cfg(target_os = "android")]
        let window = None;

        let framebuffer_size = [0, 0];
        let scale = 1.0;
        let raw_input = RawInput::default();

        #[cfg(target_arch = "wasm32")]
        let last_canvas_parent_size = resize_canvas_to_parent(
            (0, 0),
            winit_config.dom_element_id.as_ref().unwrap()
        ).unwrap();

        Self {
            event_loop: Some(event_loop),
            window: window,
            modifiers: Modifiers::default(),
            framebuffer_size,
            scale,
            cursor_pos_logical: [0.0, 0.0],
            raw_input,
            frame_events: Vec::new(),
            latest_resize_event: true,
            should_close: false,
            backend_config,
            window_builder,
            pointer_touch_id: None,
            winit_config,
            #[cfg(target_arch = "wasm32")]
            last_canvas_parent_size,
        }
    }

    fn take_raw_input(&mut self) -> egui::RawInput {
        self.raw_input.take()
    }

    fn get_window(&mut self) -> Option<&mut Self::WindowType> {
        self.window.as_mut()
    }

    fn get_live_physical_size_framebuffer(&mut self) -> Option<[u32; 2]> {
        if let Some(window) = self.window.as_ref() {
            let size = window.inner_size();
            Some([size.width, size.height])
        } else {
            None
        }
    }

    fn run_event_loop<G: GfxBackend<Self> + 'static, U: UserAppData<Self, G> + 'static>(
        mut self,
        mut gfx_backend: G,
        mut user_app: U,
    ) {
        let egui_context = egui::Context::default();
        let mut suspended = true;

        self.event_loop.take().expect("event loop missing").run(
            move |
                event,
                _event_loop,
                control_flow
            | {
                *control_flow = ControlFlow::Poll;

                match event {
                    event::Event::Suspended => {
                        suspended = true;
                        tracing::warn!("suspend event received");
                        #[cfg(not(target_os = "android"))]
                        panic!("suspend on non-android platforms is not supported at the moment");
                        #[cfg(target_os = "android")]
                        {
                            gfx_backend.suspend(&mut self);
                            self.window = None;
                        }
                    }
                    event::Event::Resumed => {
                        suspended = false;
                        tracing::warn!("resume event received");
                        #[cfg(target_os = "android")]
                        {
                            self.window = Some(
                                self.window_builder
                                    .clone()
                                    .build(_event_loop)
                                    .expect("failed to create window"),
                            );
                            gfx_backend.resume(&mut self);
                        }
                        let framebuffer_size_physical = self
                            .window
                            .as_ref()
                            .expect("failed to get size of window after resume event")
                            .inner_size();

                        self.framebuffer_size = [
                            framebuffer_size_physical.width,
                            framebuffer_size_physical.height,
                        ];
                        self.scale = self
                            .window
                            .as_ref()
                            .expect("failed to get scale of window after resume event")
                            .scale_factor() as f32;
                        let window_size =
                            framebuffer_size_physical.to_logical::<f32>(self.scale as f64);

                        let screen_size = if gfx_backend.is_rendering_to_offscreen_render_target() {
                            let (width, height) = gfx_backend.updateRenderTargetRect(
                                window_size.width as u32,
                                window_size.height as u32,
                            );
                            [
                                width as f32,
                                height as f32
                            ]

                        } else {
                            [
                                window_size.width,
                                window_size.height
                            ]
                        };

                        self.raw_input = RawInput {
                            screen_rect: Some(Rect::from_two_pos(
                                [0.0, 0.0].into(),
                                screen_size.into(),
                            )),
                            pixels_per_point: Some(self.scale),
                            ..Default::default()
                        };
                    }
                    event::Event::MainEventsCleared => {
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw()
                        }
                    }
                    event::Event::RedrawRequested(_) => {
                        if !suspended {
                            #[cfg(target_arch = "wasm32")]
                            {
                                let last_canvas_parent_size = resize_canvas_to_parent(
                                    self.last_canvas_parent_size,
                                    self.winit_config.dom_element_id.as_ref().unwrap()
                                );

                                if last_canvas_parent_size.is_some() {
                                    self.last_canvas_parent_size = last_canvas_parent_size.unwrap();
                                    let event = event::Event::WindowEvent{
                                        window_id: self.window.as_ref().unwrap().id(),
                                        event: event::WindowEvent::Resized(
                                            PhysicalSize {
                                                width: self.last_canvas_parent_size.0 as u32,
                                                height: self.last_canvas_parent_size.1 as u32,
                                            }
                                        )
                                    };

                                    self.handle_event(
                                        event,
                                        &mut gfx_backend
                                    );
                                }
                            }

                            // take egui input
                            let input = self.take_raw_input();

                            // prepare surface for drawing
                            gfx_backend.prepare_frame(
                                self.latest_resize_event,
                                &mut self
                            );
                            self.latest_resize_event = false;
                            // begin egui with input
                            // run userapp gui function. let user do anything he wants with window or gfx backends
                            let output =
                                user_app.run(
                                    &egui_context,
                                    input,
                                    &mut self,
                                    &mut gfx_backend
                                );

                            let screen_size_logical = if gfx_backend.is_rendering_to_offscreen_render_target() {
                                let (width, height) = gfx_backend.updateRenderTargetRect(
                                    self.framebuffer_size[0],
                                    self.framebuffer_size[1]
                                );
                                [
                                    width as f32 / self.scale,
                                    height as f32 / self.scale,
                                ]
                            } else {
                                [
                                    self.framebuffer_size[0] as f32 / self.scale,
                                    self.framebuffer_size[1] as f32 / self.scale,
                                ]
                            };

                            // prepare egui render data for gfx backend
                            let egui_gfx_data = EguiGfxData {
                                meshes: egui_context.tessellate(output.shapes),
                                textures_delta: output.textures_delta,
                                screen_size_logical,
                            };

                            if gfx_backend.is_rendering_to_offscreen_render_target() {
                                // render egui with gfx backend first
                                gfx_backend.render(egui_gfx_data);

                                // render final pass of user app using already rendered egui second
                                user_app.run_final_pass(&mut gfx_backend);
                            } else {
                                // render final pass of user app first
                                user_app.run_final_pass(&mut gfx_backend);

                                // render egui with gfx backend on top of final 3D pass
                                gfx_backend.render(egui_gfx_data);
                            }

                            // present the frame and loop back
                            gfx_backend.present(&mut self);
                        }
                    }
                    rest => self.handle_event(rest, &mut gfx_backend),
                }
                if self.should_close {
                    *control_flow = ControlFlow::Exit;
                }
            },
        )
    }

    fn get_config(&self) -> &BackendConfig {
        &self.backend_config
    }

    fn swap_buffers(&mut self) {
        unimplemented!("winit backend doesn't support swapping buffers")
    }

    fn get_proc_address(&mut self, _: &str) -> *const core::ffi::c_void {
        unimplemented!("winit backend doesn't support loading opengl function pointers")
    }
}

impl WinitBackend {
    fn handle_event<G: GfxBackend<Self> + 'static> (
        &mut self,
        event: winit::event::Event<()>,
        gfx_backend: &mut G,
    ) {
        if let Some(egui_event) = match event {
            event::Event::WindowEvent { event, .. } => match event {
                event::WindowEvent::Resized(size) => {
                    tracing::warn!("Resized");

                    let logical_size = if gfx_backend.is_rendering_to_offscreen_render_target() {
                        let (width, height) = gfx_backend.updateRenderTargetRect(
                            size.width,
                            size.height
                        );
                        [
                            width as f32 / self.scale,
                            height as f32 / self.scale,
                        ]

                    } else {
                        [
                            size.width as f32 / self.scale,
                            size.height as f32 / self.scale,
                        ]
                    };

                    tracing::warn!("Resized {:?}", size);


                    self.raw_input.screen_rect = Some(Rect::from_two_pos(
                        Default::default(),
                        [logical_size[0], logical_size[1]].into(),
                    ));
                    self.latest_resize_event = true;
                    self.framebuffer_size = size.into();
                    None
                }
                event::WindowEvent::CloseRequested => {
                    self.should_close = true;
                    None
                }
                event::WindowEvent::DroppedFile(df) => {
                    self.raw_input.dropped_files.push(DroppedFile {
                        path: Some(df.clone()),
                        name: df
                            .file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or_default()
                            .to_string(),
                        last_modified: None,
                        bytes: None,
                    });
                    None
                }

                event::WindowEvent::ReceivedCharacter(c) => Some(Event::Text(c.to_string())),

                event::WindowEvent::KeyboardInput { input, .. } => {
                    let pressed = match input.state {
                        event::ElementState::Pressed => true,
                        event::ElementState::Released => false,
                    };
                    if let Some(key_code) = input.virtual_keycode {
                        if let Some(egui_key) = winit_key_to_egui(key_code) {
                            Some(Event::Key {
                                key: egui_key,
                                pressed,
                                modifiers: self.modifiers,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                event::WindowEvent::ModifiersChanged(modifiers) => {
                    self.modifiers = winit_modifiers_to_egui(modifiers);
                    None
                }
                event::WindowEvent::CursorMoved { position, .. } => {
                    let logical = position.to_logical::<f32>(self.scale as f64);
                    let (x, y) = gfx_backend.mouse_pos_screen_to_render_target_space(logical.x, logical.y);

                    self.cursor_pos_logical = [x, y];
                    //tracing::warn!("Cursor moved x, y {} {}", x, y);
                    Some(Event::PointerMoved([x, y].into()))
                }
                event::WindowEvent::CursorLeft { .. } => Some(Event::PointerGone),
                event::WindowEvent::MouseWheel { delta, .. } => match delta {
                    event::MouseScrollDelta::LineDelta(x, y) => Some(Event::Scroll([x, y].into())),
                    event::MouseScrollDelta::PixelDelta(pos) => {
                        let lpos = pos.to_logical::<f32>(self.scale as f64);
                        Some(Event::Scroll([lpos.x, lpos.y].into()))
                    }
                },
                event::WindowEvent::MouseInput { state, button, .. } => {
                    let pressed = match state {
                        event::ElementState::Pressed => true,
                        event::ElementState::Released => false,
                    };
                    Some(Event::PointerButton {
                        pos: self.cursor_pos_logical.into(),
                        button: winit_mouse_button_to_egui(button),
                        pressed,
                        modifiers: self.modifiers,
                    })
                }
                event::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    self.scale = scale_factor as f32;
                    self.raw_input.pixels_per_point = Some(scale_factor as f32);
                    self.latest_resize_event = true;
                    None
                }

                event::WindowEvent::Destroyed => {
                    tracing::warn!("window destroyed");
                    None
                }
                event::WindowEvent::Touch(touch) => {
                    // code stolen from eframe(egui-winit).


                    let pos = egui::pos2(
                        touch.location.x as f32 / self.scale,
                        touch.location.y as f32 / self.scale,
                    );
                    tracing::warn!("touch event: {} {}", touch.location.x, touch.location.y);
                    self.cursor_pos_logical = [pos.x, pos.y];

                    if self.pointer_touch_id.is_none() || self.pointer_touch_id.unwrap() == touch.id
                    {
                        // … emit PointerButton resp. PointerMoved events to emulate mouse
                        match touch.phase {
                            winit::event::TouchPhase::Started => {
                                self.pointer_touch_id = Some(touch.id);
                                // First move the pointer to the right location

                                self.raw_input.events.push(Event::PointerMoved(pos));
                                self.raw_input.events.push(Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: self.modifiers,
                                });
                            }
                            winit::event::TouchPhase::Moved => {
                                self.raw_input.events.push(Event::PointerMoved(pos));
                            }
                            winit::event::TouchPhase::Ended => {
                                self.pointer_touch_id = None;
                                self.raw_input.events.push(Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: false,
                                    modifiers: self.modifiers,
                                });
                                self.raw_input.events.push(egui::Event::PointerGone);
                            }
                            winit::event::TouchPhase::Cancelled => {
                                self.pointer_touch_id = None;

                                self.raw_input.events.push(egui::Event::PointerGone);
                            }
                        }
                    }
                    Some(Event::Touch {
                        device_id: egui::TouchDeviceId(egui::epaint::util::hash(touch.device_id)),
                        id: egui::TouchId::from(touch.id),
                        phase: match touch.phase {
                            winit::event::TouchPhase::Started => egui::TouchPhase::Start,
                            winit::event::TouchPhase::Moved => egui::TouchPhase::Move,
                            winit::event::TouchPhase::Ended => egui::TouchPhase::End,
                            winit::event::TouchPhase::Cancelled => egui::TouchPhase::Cancel,
                        },
                        pos,
                        force: match touch.force {
                            Some(winit::event::Force::Normalized(force)) => force as f32,
                            Some(winit::event::Force::Calibrated {
                                force,
                                max_possible_force,
                                ..
                            }) => (force / max_possible_force) as f32,
                            None => 0_f32,
                        },
                    })
                }
                _ => None,
            },
            _ => None,
        } {
            self.raw_input.events.push(egui_event);
        }
    }
}

fn winit_modifiers_to_egui(modifiers: ModifiersState) -> Modifiers {
    Modifiers {
        alt: modifiers.alt(),
        ctrl: modifiers.ctrl(),
        shift: modifiers.shift(),
        // i have no idea what a mac_cmd key is
        mac_cmd: false,
        command: modifiers.logo(),
    }
}
fn winit_mouse_button_to_egui(mb: winit::event::MouseButton) -> egui::PointerButton {
    match mb {
        MouseButton::Left => egui::PointerButton::Primary,
        MouseButton::Right => egui::PointerButton::Secondary,
        MouseButton::Middle => egui::PointerButton::Middle,
        MouseButton::Other(_) => egui::PointerButton::Extra1,
    }
}
fn winit_key_to_egui(key_code: VirtualKeyCode) -> Option<Key> {
    let key = match key_code {
        VirtualKeyCode::Down => Key::ArrowDown,
        VirtualKeyCode::Left => Key::ArrowLeft,
        VirtualKeyCode::Right => Key::ArrowRight,
        VirtualKeyCode::Up => Key::ArrowUp,

        VirtualKeyCode::Escape => Key::Escape,
        VirtualKeyCode::Tab => Key::Tab,
        VirtualKeyCode::Back => Key::Backspace,
        VirtualKeyCode::Return => Key::Enter,
        VirtualKeyCode::Space => Key::Space,

        VirtualKeyCode::Insert => Key::Insert,
        VirtualKeyCode::Delete => Key::Delete,
        VirtualKeyCode::Home => Key::Home,
        VirtualKeyCode::End => Key::End,
        VirtualKeyCode::PageUp => Key::PageUp,
        VirtualKeyCode::PageDown => Key::PageDown,

        VirtualKeyCode::Key0 | VirtualKeyCode::Numpad0 => Key::Num0,
        VirtualKeyCode::Key1 | VirtualKeyCode::Numpad1 => Key::Num1,
        VirtualKeyCode::Key2 | VirtualKeyCode::Numpad2 => Key::Num2,
        VirtualKeyCode::Key3 | VirtualKeyCode::Numpad3 => Key::Num3,
        VirtualKeyCode::Key4 | VirtualKeyCode::Numpad4 => Key::Num4,
        VirtualKeyCode::Key5 | VirtualKeyCode::Numpad5 => Key::Num5,
        VirtualKeyCode::Key6 | VirtualKeyCode::Numpad6 => Key::Num6,
        VirtualKeyCode::Key7 | VirtualKeyCode::Numpad7 => Key::Num7,
        VirtualKeyCode::Key8 | VirtualKeyCode::Numpad8 => Key::Num8,
        VirtualKeyCode::Key9 | VirtualKeyCode::Numpad9 => Key::Num9,

        VirtualKeyCode::A => Key::A,
        VirtualKeyCode::B => Key::B,
        VirtualKeyCode::C => Key::C,
        VirtualKeyCode::D => Key::D,
        VirtualKeyCode::E => Key::E,
        VirtualKeyCode::F => Key::F,
        VirtualKeyCode::G => Key::G,
        VirtualKeyCode::H => Key::H,
        VirtualKeyCode::I => Key::I,
        VirtualKeyCode::J => Key::J,
        VirtualKeyCode::K => Key::K,
        VirtualKeyCode::L => Key::L,
        VirtualKeyCode::M => Key::M,
        VirtualKeyCode::N => Key::N,
        VirtualKeyCode::O => Key::O,
        VirtualKeyCode::P => Key::P,
        VirtualKeyCode::Q => Key::Q,
        VirtualKeyCode::R => Key::R,
        VirtualKeyCode::S => Key::S,
        VirtualKeyCode::T => Key::T,
        VirtualKeyCode::U => Key::U,
        VirtualKeyCode::V => Key::V,
        VirtualKeyCode::W => Key::W,
        VirtualKeyCode::X => Key::X,
        VirtualKeyCode::Y => Key::Y,
        VirtualKeyCode::Z => Key::Z,

        VirtualKeyCode::F1 => Key::F1,
        VirtualKeyCode::F2 => Key::F2,
        VirtualKeyCode::F3 => Key::F3,
        VirtualKeyCode::F4 => Key::F4,
        VirtualKeyCode::F5 => Key::F5,
        VirtualKeyCode::F6 => Key::F6,
        VirtualKeyCode::F7 => Key::F7,
        VirtualKeyCode::F8 => Key::F8,
        VirtualKeyCode::F9 => Key::F9,
        VirtualKeyCode::F10 => Key::F10,
        VirtualKeyCode::F11 => Key::F11,
        VirtualKeyCode::F12 => Key::F12,
        VirtualKeyCode::F13 => Key::F13,
        VirtualKeyCode::F14 => Key::F14,
        VirtualKeyCode::F15 => Key::F15,
        VirtualKeyCode::F16 => Key::F16,
        VirtualKeyCode::F17 => Key::F17,
        VirtualKeyCode::F18 => Key::F18,
        VirtualKeyCode::F19 => Key::F19,
        VirtualKeyCode::F20 => Key::F20,
        _ => return None,
    };
    Some(key)
}

#[cfg(target_arch = "wasm32")]
fn resize_canvas_to_parent(last_size: (i32, i32), canvas_id: &str /*, max_size_points: egui::Vec2*/) -> Option<(i32, i32)> {
    let canvas = canvas_element(canvas_id)?;
    let parent = canvas.parent_element()?;

    let width = parent.client_width();
    let height = parent.client_height();

    // no change in resolution
    if last_size.0 == width && last_size.1 == height {
        return None;
    }

    tracing::info!("resize_canvas_to_parent {}, {}", width, height);

    let canvas_real_size = egui::Vec2 {
        x: width as f32,
        y: height as f32,
    };

    if width <= 0 || height <= 0 {
        tracing::error!("egui canvas parent size is {}x{}. Try adding `html, body {{ height: 100%; width: 100% }}` to your CSS!", width, height);
    }

    let pixels_per_point = native_pixels_per_point();

    //let max_size_pixels = pixels_per_point * max_size_points;

    let canvas_size_pixels = canvas_real_size * pixels_per_point;
    //let canvas_size_pixels = canvas_size_pixels.min(max_size_pixels);
    let canvas_size_points = canvas_size_pixels / pixels_per_point;

    // Make sure that the height and width are always even numbers.
    // otherwise, the page renders blurry on some platforms.
    // See https://github.com/emilk/egui/issues/103
    fn round_to_even(v: f32) -> f32 {
        (v / 2.0).round() * 2.0
    }

    canvas
        .style()
        .set_property(
            "width",
            &format!("{}px", round_to_even(canvas_size_points.x)),
        )
        .ok()?;
    canvas
        .style()
        .set_property(
            "height",
            &format!("{}px", round_to_even(canvas_size_points.y)),
        )
        .ok()?;
    canvas.set_width(round_to_even(canvas_size_pixels.x) as u32);
    canvas.set_height(round_to_even(canvas_size_pixels.y) as u32);

    Some((width, height))
}

#[cfg(target_arch = "wasm32")]
pub fn canvas_element(canvas_id: &str) -> Option<web_sys::HtmlCanvasElement> {
    use wasm_bindgen::JsCast;
    let document = web_sys::window()?.document()?;
    let canvas = document.get_element_by_id(canvas_id)?;
    canvas.dyn_into::<web_sys::HtmlCanvasElement>().ok()
}

#[cfg(target_arch = "wasm32")]
pub fn native_pixels_per_point() -> f32 {
    let pixels_per_point = web_sys::window().unwrap().device_pixel_ratio() as f32;
    if pixels_per_point > 0.0 && pixels_per_point.is_finite() {
        pixels_per_point
    } else {
        1.0
    }
}