//! mythterm: GPU-accelerated terminal emulator entry point.
//!
//! Initializes the configuration, font system, multiplexer, renderer,
//! and egui UI, then runs the main event loop.

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;

use egui::Context;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use mythterm_config::{
    load_settings, ensure_config_exists, MythtermConfig, Settings,
};
use mythterm_font::FontMetrics;
use mythterm_mux::Mux;
use mythterm_render::TerminalRenderer;
use mythterm_ui::input::InputMapper;
use mythterm_ui::overlay::{CommandPalette, SearchOverlay, SearchAction};
use mythterm_ui::tabbar::TabBar;
use mythterm_ui::terminal_widget::TerminalWidget;
use mythterm_ui::AppState;

#[derive(Parser, Debug)]
#[command(name = "mythterm", about = "GPU-accelerated terminal emulator")]
struct Args {
    /// Skip loading config file
    #[arg(long, short = 'n')]
    skip_config: bool,

    /// Initial working directory
    #[arg(long, short = 'd')]
    cwd: Option<String>,
}

/// The main application state.
struct MythtermApp {
    /// Window (set after resume).
    window: Option<Arc<Window>>,
    /// egui context.
    egui_ctx: Option<Context>,
    /// egui winit state.
    egui_state: Option<egui_winit::State>,
    /// Terminal renderer.
    renderer: Option<TerminalRenderer>,
    /// Multiplexer.
    mux: Arc<Mux>,
    /// Input mapper.
    input_mapper: InputMapper,
    /// Application state (UI).
    app_state: AppState,
    /// Configuration.
    config: Arc<MythtermConfig>,
    /// Font metrics.
    metrics: FontMetrics,
    /// Search overlay.
    search: SearchOverlay,
    /// Command palette.
    command_palette: CommandPalette,
    /// Arguments.
    args: Args,
}

impl MythtermApp {
    fn new(args: Args) -> Result<Self> {
        // Load config
        let settings = if args.skip_config {
            Settings::default()
        } else {
            ensure_config_exists()?;
            load_settings()?
        };

        let config = Arc::new(MythtermConfig::new(settings));
        let mux = Arc::new(Mux::new());
        let input_mapper = InputMapper::new();
        let metrics = FontMetrics::default();

        Ok(Self {
            window: None,
            egui_ctx: None,
            egui_state: None,
            renderer: None,
            mux,
            input_mapper,
            app_state: AppState::default(),
            config,
            metrics,
            search: SearchOverlay::new(),
            command_palette: CommandPalette::new(),
            args,
        })
    }
}

impl ApplicationHandler for MythtermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Create window
        let attrs = Window::default_attributes()
            .with_title("mythterm")
            .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));
        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));

        // Create wgpu instance, adapter, device, queue
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).expect("Failed to create surface");

        // Block on async adapter/device creation
        let (device, queue, format) = pollster::block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .expect("Failed to find GPU adapter");

            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("mythterm"),
                    ..Default::default()
                })
                .await
                .expect("Failed to create device");

            let caps = surface.get_capabilities(&adapter);
            let format = caps.formats[0];

            (device, queue, format)
        });

        // Create renderer
        let size = window.inner_size();
        let mut renderer = TerminalRenderer::new(device, queue, format, size.width, size.height)
            .expect("Failed to create renderer");

        // Load default font
        if let Err(e) = renderer.load_font("monospace", false, false) {
            log::warn!("Failed to load default font: {}", e);
        }

        // Create egui context and state
        let egui_ctx = Context::default();
        let viewport_id = egui_ctx.viewport_id();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            viewport_id,
            &window,
            None,
            None,
            None,
        );

        self.window = Some(window);
        self.egui_ctx = Some(egui_ctx);
        self.egui_state = Some(egui_state);
        self.renderer = Some(renderer);

        log::info!("Window created, renderer initialized");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Feed event to egui
        if let (Some(state), Some(window)) = (&mut self.egui_state, &self.window) {
            let _ = state.on_window_event(window, &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() {
                    // Get logical key from winit event
                    let key = &event.logical_key;
                    let modifiers = self.egui_state.as_ref()
                        .map(|s| s.egui_input().modifiers)
                        .unwrap_or_default();

                    let text = event.text.as_deref();

                    // Check for app-level shortcuts first
                    if modifiers.ctrl && modifiers.shift {
                        // Map winit key to egui key for shortcuts
                        let egui_key = winit_key_to_egui(key);
                        if let Some(ek) = egui_key {
                            match ek {
                                egui::Key::F => {
                                    self.search = SearchOverlay::new();
                                    self.app_state.search_open = true;
                                    return;
                                }
                                egui::Key::P => {
                                    self.command_palette = CommandPalette::new();
                                    self.app_state.command_palette_open = true;
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }

                    // Map key to VT sequence and send to terminal
                    let egui_key = winit_key_to_egui(key);
                    if let Some(ek) = egui_key {
                        if let Some(vt_seq) = self.input_mapper.map_key(ek, &modifiers, text) {
                            // TODO: Send to active pane's PTY
                            log::debug!("VT sequence: {:?}", String::from_utf8_lossy(&vt_seq));
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Convert winit key to egui key.
fn winit_key_to_egui(key: &winit::keyboard::Key) -> Option<egui::Key> {
    use winit::keyboard::Key;
    match key {
        Key::Named(winit::keyboard::NamedKey::ArrowUp) => Some(egui::Key::ArrowUp),
        Key::Named(winit::keyboard::NamedKey::ArrowDown) => Some(egui::Key::ArrowDown),
        Key::Named(winit::keyboard::NamedKey::ArrowLeft) => Some(egui::Key::ArrowLeft),
        Key::Named(winit::keyboard::NamedKey::ArrowRight) => Some(egui::Key::ArrowRight),
        Key::Named(winit::keyboard::NamedKey::Enter) => Some(egui::Key::Enter),
        Key::Named(winit::keyboard::NamedKey::Tab) => Some(egui::Key::Tab),
        Key::Named(winit::keyboard::NamedKey::Backspace) => Some(egui::Key::Backspace),
        Key::Named(winit::keyboard::NamedKey::Escape) => Some(egui::Key::Escape),
        Key::Named(winit::keyboard::NamedKey::Home) => Some(egui::Key::Home),
        Key::Named(winit::keyboard::NamedKey::End) => Some(egui::Key::End),
        Key::Named(winit::keyboard::NamedKey::PageUp) => Some(egui::Key::PageUp),
        Key::Named(winit::keyboard::NamedKey::PageDown) => Some(egui::Key::PageDown),
        Key::Named(winit::keyboard::NamedKey::Insert) => Some(egui::Key::Insert),
        Key::Named(winit::keyboard::NamedKey::Delete) => Some(egui::Key::Delete),
        Key::Named(winit::keyboard::NamedKey::F1) => Some(egui::Key::F1),
        Key::Named(winit::keyboard::NamedKey::F2) => Some(egui::Key::F2),
        Key::Named(winit::keyboard::NamedKey::F3) => Some(egui::Key::F3),
        Key::Named(winit::keyboard::NamedKey::F4) => Some(egui::Key::F4),
        Key::Named(winit::keyboard::NamedKey::F5) => Some(egui::Key::F5),
        Key::Named(winit::keyboard::NamedKey::F6) => Some(egui::Key::F6),
        Key::Named(winit::keyboard::NamedKey::F7) => Some(egui::Key::F7),
        Key::Named(winit::keyboard::NamedKey::F8) => Some(egui::Key::F8),
        Key::Named(winit::keyboard::NamedKey::F9) => Some(egui::Key::F9),
        Key::Named(winit::keyboard::NamedKey::F10) => Some(egui::Key::F10),
        Key::Named(winit::keyboard::NamedKey::F11) => Some(egui::Key::F11),
        Key::Named(winit::keyboard::NamedKey::F12) => Some(egui::Key::F12),
        Key::Character(c) => {
            if c.len() == 1 {
                let ch = c.chars().next().unwrap();
                match ch {
                    'a'..='z' => Some(egui::Key::A), // simplified
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

impl MythtermApp {
    fn render(&mut self) {
        let (Some(egui_ctx), Some(egui_state), Some(window)) =
            (&self.egui_ctx, &mut self.egui_state, &self.window)
        else {
            return;
        };

        // Run egui frame
        let raw_input = egui_state.take_egui_input(window);
        let full_output = egui_ctx.run_ui(raw_input, |ctx| {
            // Tab bar at top
            egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
                let tab_bar = TabBar::new(self.app_state.tab_titles.clone(), self.app_state.active_tab);
                if let Some(clicked) = tab_bar.show(ui) {
                    if clicked < self.app_state.tab_titles.len() {
                        self.app_state.active_tab = clicked;
                    } else {
                        // New tab
                        self.app_state.tab_titles.push(format!("Tab {}", self.app_state.tab_titles.len() + 1));
                        self.app_state.active_tab = self.app_state.tab_titles.len() - 1;
                    }
                }
            });

            // Terminal content area
            egui::CentralPanel::default().show(ctx, |ui| {
                let widget = TerminalWidget::new(
                    80,
                    24,
                    self.metrics.cell_width,
                    self.metrics.cell_height,
                );
                ui.add(widget);
            });

            // Search overlay
            if self.app_state.search_open {
                match self.search.show(ctx) {
                    SearchAction::Close => {
                        self.app_state.search_open = false;
                    }
                    SearchAction::QueryChanged => {
                        // TODO: search in terminal output
                    }
                    _ => {}
                }
            }

            // Command palette
            if self.app_state.command_palette_open {
                if let Some(cmd) = self.command_palette.show(ctx) {
                    self.app_state.command_palette_open = false;
                    log::info!("Command selected: {}", cmd);
                    // TODO: execute command
                }
            }
        });

        egui_state.handle_platform_output(window, full_output.platform_output);

        // TODO: Render terminal content via TerminalRenderer
        // This requires integrating with the wgpu surface

        window.request_redraw();
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    log::info!("mythterm v{} starting", env!("CARGO_PKG_VERSION"));

    let mut app = MythtermApp::new(args)?;
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;

    Ok(())
}
