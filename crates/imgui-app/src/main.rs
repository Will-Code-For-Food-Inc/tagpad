//! Native front end: window, GL context, `ImGui`, and a controller.
//!
//! Everything that decides what happens lives in `tagpad_core`. This binary
//! opens a window, copies raw controller values into the shared input state
//! machine, and draws whatever the session says to draw.

#[allow(unreachable_pub)]
mod cli;
#[allow(unreachable_pub)]
mod pad;
#[allow(unreachable_pub)]
mod ui;

use anyhow::{Context as _, Result, anyhow};
use clap::Parser;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext as _, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay as _, GlDisplay as _};
use glutin::surface::{GlSurface as _, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow as _};
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use raw_window_handle::HasWindowHandle as _;
use std::num::NonZeroU32;
use std::path::PathBuf;
use tagpad_core::input::{Gamepad, action_for, action_for_key};
use tagpad_core::{Decisions, Session, Task};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

fn main() -> Result<()> {
    let args = cli::Cli::parse();
    let task: Task = serde_json::from_str(
        &std::fs::read_to_string(&args.task)
            .with_context(|| format!("reading {}", args.task.display()))?,
    )
    .context("parsing the task file")?;

    let out = args.out_path();
    // Resuming is the default: labelling happens in stolen half-hours, and
    // silently discarding earlier work is the fastest way to lose a labeller.
    let saved: Decisions = if args.fresh {
        Decisions::new()
    } else {
        std::fs::read_to_string(&out)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| serde_json::from_value(v.get("verdicts")?.clone()).ok())
            .unwrap_or_default()
    };
    if !saved.is_empty() {
        println!(
            "resuming: {} of {} already recorded",
            saved.len(),
            task.cards.len()
        );
    }

    let session = Session::new(task, saved).ok_or_else(|| anyhow!("the task has no cards"))?;

    let event_loop = EventLoop::new().context("creating the event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(session, out, args.reviewer);
    event_loop
        .run_app(&mut app)
        .context("running the event loop")?;
    app.save()?;
    Ok(())
}

struct Gl {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    platform: WinitPlatform,
    imgui: imgui::Context,
    renderer: imgui_glow_renderer::AutoRenderer,
}

struct App {
    session: Session,
    out: PathBuf,
    reviewer: String,
    gamepad: Gamepad,
    pad: pad::Pad,
    gl: Option<Gl>,
    dirty: bool,
}

impl App {
    fn new(session: Session, out: PathBuf, reviewer: String) -> Self {
        Self {
            session,
            out,
            reviewer,
            gamepad: Gamepad::new(),
            pad: pad::Pad::open(),
            gl: None,
            dirty: false,
        }
    }

    /// Write after every recorded judgment, not on exit. A crash or a closed
    /// lid must not cost the labeller their session.
    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.session.output(&self.reviewer))?;
        std::fs::write(&self.out, json)
            .with_context(|| format!("writing {}", self.out.display()))?;
        Ok(())
    }

    fn apply(&mut self, action: tagpad_core::Action) {
        if self.session.apply(action).is_some() {
            self.dirty = true;
        }
    }

    fn init(event_loop: &ActiveEventLoop) -> Result<Gl> {
        let attrs = Window::default_attributes()
            .with_title("tagpad")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 800));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(event_loop, ConfigTemplateBuilder::new(), |mut c| {
                c.next()
                    .unwrap_or_else(|| unreachable!("glutin yields at least one config"))
            })
            .map_err(|e| anyhow!("no usable GL config: {e}"))?;
        let window = window.ok_or_else(|| anyhow!("no window"))?;

        let raw = window.window_handle()?.as_raw();
        let display = config.display();
        let context = unsafe {
            display.create_context(&config, &ContextAttributesBuilder::new().build(Some(raw)))?
        };
        let surface = unsafe {
            display
                .create_window_surface(&config, &window.build_surface_attributes(<_>::default())?)?
        };
        let context = context.make_current(&surface)?;
        // Without vsync this spins the GPU at full tilt to redraw a static
        // page of text -- on a handheld that is battery for nothing.
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s).cast())
        };

        let mut imgui = imgui::Context::create();
        imgui.set_ini_filename(None);
        imgui.style_mut().window_rounding = 0.0;
        imgui.style_mut().frame_rounding = 6.0;
        imgui.style_mut().frame_padding = [10.0, 8.0];
        imgui.style_mut().item_spacing = [8.0, 8.0];

        let mut platform = WinitPlatform::new(&mut imgui);
        platform.attach_window(imgui.io_mut(), &window, HiDpiMode::Default);

        let renderer = imgui_glow_renderer::AutoRenderer::new(gl, &mut imgui)
            .map_err(|e| anyhow!("renderer: {e}"))?;

        Ok(Gl {
            window,
            surface,
            context,
            platform,
            imgui,
            renderer,
        })
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gl.is_some() {
            return;
        }
        match Self::init(event_loop) {
            Ok(gl) => self.gl = Some(gl),
            Err(e) => {
                eprintln!("could not open a window: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(gl) = &mut self.gl else { return };
        gl.platform.handle_event::<()>(
            gl.imgui.io_mut(),
            &gl.window,
            &winit::event::Event::WindowEvent {
                window_id: gl.window.id(),
                event: event.clone(),
            },
        );

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    gl.surface.resize(&gl.context, w, h);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // Names match what a browser reports, so one binding table in
                // core serves both builds.
                let name = match &event.logical_key {
                    Key::Character(c) => c.to_lowercase(),
                    Key::Named(NamedKey::Enter) => "enter".into(),
                    Key::Named(NamedKey::Escape) => "escape".into(),
                    Key::Named(NamedKey::ArrowUp) => "arrowup".into(),
                    Key::Named(NamedKey::ArrowDown) => "arrowdown".into(),
                    Key::Named(NamedKey::ArrowLeft) => "arrowleft".into(),
                    Key::Named(NamedKey::ArrowRight) => "arrowright".into(),
                    _ => return,
                };
                if let Some(action) = action_for_key(&name, self.session.view().mode) {
                    self.apply(action);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.render() {
                    eprintln!("draw failed: {e:#}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        // Controller state is polled, not delivered as events, so a frame has
        // to be driven even when the window is idle.
        let pressed = {
            let (buttons, axes) = self.pad.poll();
            self.gamepad.frame(buttons, axes)
        };
        for button in pressed {
            if let Some(action) = action_for(button, self.session.view().mode) {
                self.apply(action);
            }
        }
        if self.dirty {
            self.dirty = false;
            if let Err(e) = self.save() {
                eprintln!("could not save: {e:#}");
            }
        }
        if let Some(gl) = &self.gl {
            gl.window.request_redraw();
        }
    }
}

impl App {
    fn render(&mut self) -> Result<()> {
        let name = self.pad.name();
        let Some(gl) = &mut self.gl else {
            return Ok(());
        };
        gl.platform
            .prepare_frame(gl.imgui.io_mut(), &gl.window)
            .map_err(|e| anyhow!("prepare_frame: {e}"))?;

        // One frame per render: new_frame() *starts* a frame, so calling it
        // twice would throw away everything drawn into the first one.
        let ui = gl.imgui.new_frame();
        let action = ui::draw(ui, &self.session, name.as_deref());
        gl.platform.prepare_render(ui, &gl.window);
        let draw_data = gl.imgui.render();
        gl.renderer
            .render(draw_data)
            .map_err(|e| anyhow!("render: {e}"))?;
        gl.surface.swap_buffers(&gl.context)?;

        if let Some(action) = action {
            self.apply(action);
        }
        Ok(())
    }
}
