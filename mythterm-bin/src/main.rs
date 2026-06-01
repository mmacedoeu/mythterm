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
use mythterm_mux::domain::{Domain, LocalDomain};
use mythterm_mux::pane::{Pane, PaneId};
use mythterm_mux::tab::Tab;
use mythterm_mux::Mux;
use mythterm_render::TerminalRenderer;
use mythterm_ui::clipboard::{ClipboardHandler, PlatformClipboard};
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
    /// Local domain for spawning panes.
    local_domain: Option<LocalDomain>,
    /// Active pane ID.
    active_pane: Option<PaneId>,
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
    /// Clipboard handler.
    clipboard: ClipboardHandler,
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
        let clipboard = ClipboardHandler::new(Arc::new(PlatformClipboard::new()));

        Ok(Self {
            window: None,
            egui_ctx: None,
            egui_state: None,
            renderer: None,
            mux,
            local_domain: None,
            active_pane: None,
            input_mapper,
            app_state: AppState::default(),
            config,
            metrics,
            search: SearchOverlay::new(),
            command_palette: CommandPalette::new(),
            clipboard,
            args,
        })
    }

    /// Spawn a new terminal pane.
    fn spawn_pane(&mut self) -> Result<PaneId> {
        let domain = self.local_domain.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Local domain not initialized"))?;

        let pane_id = self.mux.alloc_pane_id();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pane = domain.spawn(pane_id, size, None)?;

        // Create a tab for this pane
        let tab_id = self.mux.alloc_tab_id();
        let tab = Tab::new(tab_id, pane.clone());

        self.mux.insert_pane(pane);
        self.mux.insert_tab(Arc::new(tab));

        self.active_pane = Some(pane_id);
        self.app_state.tab_titles.push(format!("Tab {}", tab_id + 1));
        self.app_state.active_tab = self.app_state.tab_titles.len() - 1;

        log::info!("Spawned pane {} in tab {}", pane_id, tab_id);
        Ok(pane_id)
    }

    /// Send input to the active pane.
    fn send_input(&self, data: Vec<u8>) {
        if let Some(pane_id) = self.active_pane {
            if let Some(pane) = self.mux.get_pane(pane_id) {
                pane.write_to_pty(data);
            }
        }
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
            log::warn!("Failed to load monospace font: {}, trying fallback", e);
            // Try common font names
            for name in &["DejaVu Sans Mono", "Liberation Mono", "Consolas", "Menlo", "Courier New"] {
                if renderer.load_font(name, false, false).is_ok() {
                    log::info!("Loaded font: {}", name);
                    break;
                }
            }
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

        // Initialize local domain
        let local_domain = LocalDomain::new(0, self.config.clone(), self.config.clone());
        self.local_domain = Some(local_domain);

        self.window = Some(window);
        self.egui_ctx = Some(egui_ctx);
        self.egui_state = Some(egui_state);
        self.renderer = Some(renderer);

        // Spawn initial terminal pane
        if let Err(e) = self.spawn_pane() {
            log::error!("Failed to spawn initial pane: {}", e);
        }
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
                    let key = &event.logical_key;
                    let modifiers = self.egui_state.as_ref()
                        .map(|s| s.egui_input().modifiers)
                        .unwrap_or_default();

                    let text = event.text.as_deref();

                    // Check for app-level shortcuts first
                    if modifiers.ctrl && modifiers.shift {
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
                                egui::Key::T => {
                                    // New tab
                                    if let Err(e) = self.spawn_pane() {
                                        log::error!("Failed to spawn new tab: {}", e);
                                    }
                                    return;
                                }
                                egui::Key::W => {
                                    // Close tab - send exit to active pane
                                    self.send_input(b"exit\n".to_vec());
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }

                    // Map key to VT sequence and send to active pane
                    let egui_key = winit_key_to_egui(key);
                    if let Some(ek) = egui_key {
                        if let Some(vt_seq) = self.input_mapper.map_key(ek, &modifiers, text) {
                            self.send_input(vt_seq);
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
            let ch = c.chars().next()?;
            match ch {
                'a' => Some(egui::Key::A),
                'b' => Some(egui::Key::B),
                'c' => Some(egui::Key::C),
                'd' => Some(egui::Key::D),
                'e' => Some(egui::Key::E),
                'f' => Some(egui::Key::F),
                'g' => Some(egui::Key::G),
                'h' => Some(egui::Key::H),
                'i' => Some(egui::Key::I),
                'j' => Some(egui::Key::J),
                'k' => Some(egui::Key::K),
                'l' => Some(egui::Key::L),
                'm' => Some(egui::Key::M),
                'n' => Some(egui::Key::N),
                'o' => Some(egui::Key::O),
                'p' => Some(egui::Key::P),
                'q' => Some(egui::Key::Q),
                'r' => Some(egui::Key::R),
                's' => Some(egui::Key::S),
                't' => Some(egui::Key::T),
                'u' => Some(egui::Key::U),
                'v' => Some(egui::Key::V),
                'w' => Some(egui::Key::W),
                'x' => Some(egui::Key::X),
                'y' => Some(egui::Key::Y),
                'z' => Some(egui::Key::Z),
                _ => None,
            }
        }
        _ => None,
    }
}

impl MythtermApp {
    fn render(&mut self) {
        // Clone window Arc to avoid borrowing self.window
        let window = match &self.window {
            Some(w) => w.clone(),
            None => return,
        };

        let (Some(egui_ctx), Some(egui_state)) =
            (&self.egui_ctx, &mut self.egui_state)
        else {
            return;
        };

        // Run egui frame
        let raw_input = egui_state.take_egui_input(&window);
        let mut spawn_new_tab = false;
        let mut close_tab = false;
        let mut open_search = false;

        let full_output = egui_ctx.run_ui(raw_input, |ctx| {
            // Tab bar at top
            egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
                let tab_bar = TabBar::new(self.app_state.tab_titles.clone(), self.app_state.active_tab);
                if let Some(clicked) = tab_bar.show(ui) {
                    if clicked < self.app_state.tab_titles.len() {
                        self.app_state.active_tab = clicked;
                    } else {
                        spawn_new_tab = true;
                    }
                }
            });

            // Terminal content area
            egui::CentralPanel::default().show(ctx, |ui| {
                // Get terminal content from active pane
                if let Some(pane_id) = self.active_pane {
                    if let Some(pane) = self.mux.get_pane(pane_id) {
                        // Get terminal size
                        let size = pane.get_size();
                        let _rows = size.rows as usize;
                        let _cols = size.cols as usize;

                        // Get actual terminal content
                        let lines = pane.get_visible_lines();
                        let cursor = pane.get_cursor_position();

                        // Create terminal widget with actual content
                        let mut widget = TerminalWidget::with_content(
                            lines,
                            self.metrics.cell_width,
                            self.metrics.cell_height,
                        );
                        widget = widget.cursor(cursor.0, cursor.1);

                        ui.add(widget);
                    } else {
                        let widget = TerminalWidget::new(
                            80, 24,
                            self.metrics.cell_width,
                            self.metrics.cell_height,
                        );
                        ui.add(widget);
                    }
                } else {
                    let widget = TerminalWidget::new(
                        80, 24,
                        self.metrics.cell_width,
                        self.metrics.cell_height,
                    );
                    ui.add(widget);
                };
            });

            // Search overlay
            if self.app_state.search_open {
                match self.search.show(ctx) {
                    SearchAction::Close => {
                        self.app_state.search_open = false;
                    }
                    SearchAction::QueryChanged => {
                        // Search is handled by the overlay UI
                    }
                    _ => {}
                }
            }

            // Command palette
            if self.app_state.command_palette_open {
                if let Some(cmd) = self.command_palette.show(ctx) {
                    self.app_state.command_palette_open = false;
                    // Execute the command
                    match cmd.as_str() {
                        "New Tab" => {
                            spawn_new_tab = true;
                        }
                        "Close Tab" => {
                            close_tab = true;
                        }
                        "Search" => {
                            open_search = true;
                        }
                        _ => {
                            log::info!("Unknown command: {}", cmd);
                        }
                    }
                }
            }
        });

        // Execute deferred actions and handle egui output
        // We need to be careful with borrows since egui_state is a field of self

        // First, handle egui output
        if let (Some(egui_state), Some(window)) = (&mut self.egui_state, &self.window) {
            egui_state.handle_platform_output(window, full_output.platform_output);
        }

        // Then execute deferred actions
        if spawn_new_tab {
            if let Err(e) = self.spawn_pane() {
                log::error!("Failed to spawn new tab: {}", e);
            }
        }
        if close_tab {
            self.send_input(b"exit\n".to_vec());
        }
        if open_search {
            self.search = SearchOverlay::new();
            self.app_state.search_open = true;
        }

        // Terminal content is rendered via egui's text rendering
        // GPU-accelerated rendering via TerminalRenderer can be added
        // later by rendering to a texture and displaying as an egui Image

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
