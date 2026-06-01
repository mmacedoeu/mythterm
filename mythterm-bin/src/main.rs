//! mythterm: GPU-accelerated terminal emulator entry point.

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;

use egui::Context;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use mythterm_config::{load_settings, ensure_config_exists, MythtermConfig, Settings};
use mythterm_font::FontMetrics;
use mythterm_mux::domain::{Domain, LocalDomain};
use mythterm_mux::pane::{Pane, PaneId};
use mythterm_mux::tab::Tab;
use mythterm_mux::{Mux, MuxConfig};
use mythterm_ui::clipboard::{ClipboardHandler, PlatformClipboard};
use mythterm_ui::input::InputMapper;
use mythterm_ui::overlay::{CommandPalette, SearchOverlay, SearchAction};
use mythterm_ui::tabbar::TabBar;
use mythterm_ui::terminal_widget::TerminalWidget;
use mythterm_ui::AppState;

#[derive(Parser, Debug)]
#[command(name = "mythterm", about = "GPU-accelerated terminal emulator")]
struct Args {
    #[arg(long, short = 'n')]
    skip_config: bool,
}

/// Egui rendering state.
struct EguiRenderState {
    egui_ctx: Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    screen_descriptor: egui_wgpu::ScreenDescriptor,
}

/// The main application state.
struct MythtermApp {
    window: Option<Arc<Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    egui: Option<EguiRenderState>,
    mux: Arc<Mux>,
    local_domain: Option<LocalDomain>,
    active_pane: Option<PaneId>,
    input_mapper: InputMapper,
    app_state: AppState,
    config: Arc<MythtermConfig>,
    metrics: FontMetrics,
    search: SearchOverlay,
    command_palette: CommandPalette,
}

impl MythtermApp {
    fn new(args: Args) -> Result<Self> {
        let settings = if args.skip_config {
            Settings::default()
        } else {
            ensure_config_exists()?;
            load_settings()?
        };

        Ok(Self {
            window: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            egui: None,
            mux: Arc::new(Mux::new()),
            local_domain: None,
            active_pane: None,
            input_mapper: InputMapper::new(),
            app_state: AppState::default(),
            config: Arc::new(MythtermConfig::new(settings)),
            metrics: FontMetrics::default(),
            search: SearchOverlay::new(),
            command_palette: CommandPalette::new(),
        })
    }

    fn spawn_pane(&mut self) -> Result<PaneId> {
        let domain = self.local_domain.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Local domain not initialized"))?;

        let pane_id = self.mux.alloc_pane_id();
        let size = portable_pty::PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 };
        let pane = domain.spawn(pane_id, size, None)?;
        let tab_id = self.mux.alloc_tab_id();
        self.mux.insert_pane(pane);
        self.mux.insert_tab(Arc::new(Tab::new(tab_id, self.mux.get_pane(pane_id).unwrap())));
        self.active_pane = Some(pane_id);
        self.app_state.tab_titles.push(format!("Tab {}", tab_id + 1));
        self.app_state.active_tab = self.app_state.tab_titles.len() - 1;
        log::info!("Spawned pane {} in tab {}", pane_id, tab_id);
        Ok(pane_id)
    }

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

        let attrs = Window::default_attributes()
            .with_title("mythterm")
            .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));
        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).expect("Failed to create surface");

        let (device, queue, surface_config, format) = pollster::block_on(async {
            let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }).await.expect("Failed to find GPU adapter");

            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("mythterm"),
                ..Default::default()
            }).await.expect("Failed to create device");

            let size = window.inner_size();
            let caps = surface.get_capabilities(&adapter);
            let format = caps.formats[0];

            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &surface_config);

            (device, queue, surface_config, format)
        });

        let egui_ctx = Context::default();
        let viewport_id = egui_ctx.viewport_id();
        let egui_state = egui_winit::State::new(egui_ctx.clone(), viewport_id, &window, None, None, None);
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_config.width, surface_config.height],
            pixels_per_point: window.scale_factor() as f32,
        };

        let local_domain = LocalDomain::new(0, self.config.clone(), self.config.clone());

        self.window = Some(window);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.egui = Some(EguiRenderState { egui_ctx, state: egui_state, renderer: egui_renderer, screen_descriptor });
        self.local_domain = Some(local_domain);

        if let Err(e) = self.spawn_pane() {
            log::error!("Failed to spawn initial pane: {}", e);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(egui), Some(window)) = (&mut self.egui, &self.window) {
            let _ = egui.state.on_window_event(window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                if let (Some(device), Some(surface), Some(config)) = (&self.device, &self.surface, &mut self.surface_config) {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, config);
                }
                if let Some(egui) = &mut self.egui {
                    egui.screen_descriptor.size_in_pixels = [new_size.width, new_size.height];
                }
                if let Some(w) = &self.window { w.request_redraw(); }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() {
                    let key = &event.logical_key;
                    let modifiers = self.egui.as_ref().map(|e| e.state.egui_input().modifiers).unwrap_or_default();
                    let text = event.text.as_deref();

                    if modifiers.ctrl && modifiers.shift {
                        if let Some(ek) = winit_key_to_egui(key) {
                            match ek {
                                egui::Key::F => { self.search = SearchOverlay::new(); self.app_state.search_open = true; return; }
                                egui::Key::P => { self.command_palette = CommandPalette::new(); self.app_state.command_palette_open = true; return; }
                                egui::Key::T => { let _ = self.spawn_pane(); return; }
                                egui::Key::W => { self.send_input(b"exit\n".to_vec()); return; }
                                _ => {}
                            }
                        }
                    }

                    if let Some(ek) = winit_key_to_egui(key) {
                        if let Some(vt_seq) = self.input_mapper.map_key(ek, &modifiers, text) {
                            self.send_input(vt_seq);
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window { w.request_redraw(); }
    }
}

fn winit_key_to_egui(key: &winit::keyboard::Key) -> Option<egui::Key> {
    use winit::keyboard::{Key, NamedKey};
    match key {
        Key::Named(NamedKey::ArrowUp) => Some(egui::Key::ArrowUp),
        Key::Named(NamedKey::ArrowDown) => Some(egui::Key::ArrowDown),
        Key::Named(NamedKey::ArrowLeft) => Some(egui::Key::ArrowLeft),
        Key::Named(NamedKey::ArrowRight) => Some(egui::Key::ArrowRight),
        Key::Named(NamedKey::Enter) => Some(egui::Key::Enter),
        Key::Named(NamedKey::Tab) => Some(egui::Key::Tab),
        Key::Named(NamedKey::Backspace) => Some(egui::Key::Backspace),
        Key::Named(NamedKey::Escape) => Some(egui::Key::Escape),
        Key::Named(NamedKey::Home) => Some(egui::Key::Home),
        Key::Named(NamedKey::End) => Some(egui::Key::End),
        Key::Named(NamedKey::PageUp) => Some(egui::Key::PageUp),
        Key::Named(NamedKey::PageDown) => Some(egui::Key::PageDown),
        Key::Named(NamedKey::Insert) => Some(egui::Key::Insert),
        Key::Named(NamedKey::Delete) => Some(egui::Key::Delete),
        Key::Named(NamedKey::F1) => Some(egui::Key::F1),
        Key::Named(NamedKey::F2) => Some(egui::Key::F2),
        Key::Named(NamedKey::F3) => Some(egui::Key::F3),
        Key::Named(NamedKey::F4) => Some(egui::Key::F4),
        Key::Named(NamedKey::F5) => Some(egui::Key::F5),
        Key::Named(NamedKey::F6) => Some(egui::Key::F6),
        Key::Named(NamedKey::F7) => Some(egui::Key::F7),
        Key::Named(NamedKey::F8) => Some(egui::Key::F8),
        Key::Named(NamedKey::F9) => Some(egui::Key::F9),
        Key::Named(NamedKey::F10) => Some(egui::Key::F10),
        Key::Named(NamedKey::F11) => Some(egui::Key::F11),
        Key::Named(NamedKey::F12) => Some(egui::Key::F12),
        Key::Character(c) => {
            let ch = c.to_lowercase().chars().next()?;
            match ch {
                'a' => Some(egui::Key::A), 'b' => Some(egui::Key::B), 'c' => Some(egui::Key::C),
                'd' => Some(egui::Key::D), 'e' => Some(egui::Key::E), 'f' => Some(egui::Key::F),
                'g' => Some(egui::Key::G), 'h' => Some(egui::Key::H), 'i' => Some(egui::Key::I),
                'j' => Some(egui::Key::J), 'k' => Some(egui::Key::K), 'l' => Some(egui::Key::L),
                'm' => Some(egui::Key::M), 'n' => Some(egui::Key::N), 'o' => Some(egui::Key::O),
                'p' => Some(egui::Key::P), 'q' => Some(egui::Key::Q), 'r' => Some(egui::Key::R),
                's' => Some(egui::Key::S), 't' => Some(egui::Key::T), 'u' => Some(egui::Key::U),
                'v' => Some(egui::Key::V), 'w' => Some(egui::Key::W), 'x' => Some(egui::Key::X),
                'y' => Some(egui::Key::Y), 'z' => Some(egui::Key::Z),
                _ => None,
            }
        }
        _ => None,
    }
}

impl MythtermApp {
    fn render(&mut self) {
        let (Some(egui), Some(window), Some(device), Some(queue), Some(surface), Some(surface_config)) =
            (&mut self.egui, &self.window, &self.device, &self.queue, &self.surface, &self.surface_config)
        else { return; };

        // Begin egui frame
        let raw_input = egui.state.take_egui_input(&window);
        egui.egui_ctx.begin_pass(raw_input);

        // Build UI
        let mut spawn_new_tab = false;
        let mut close_tab = false;
        let mut open_search = false;

        // Tab bar
        egui::TopBottomPanel::top("tab_bar").show(&egui.egui_ctx, |ui| {
            let tab_bar = TabBar::new(self.app_state.tab_titles.clone(), self.app_state.active_tab);
            if let Some(clicked) = tab_bar.show(ui) {
                if clicked < self.app_state.tab_titles.len() {
                    self.app_state.active_tab = clicked;
                } else {
                    spawn_new_tab = true;
                }
            }
        });

        // Terminal content
        egui::CentralPanel::default().show(&egui.egui_ctx, |ui| {
            if let Some(pane_id) = self.active_pane {
                if let Some(pane) = self.mux.get_pane(pane_id) {
                    let lines = pane.get_visible_lines();
                    let cursor = pane.get_cursor_position();
                    let size = pane.get_size();

                    // Debug output to stderr (always visible)
                    eprintln!("[DEBUG] Pane {}: {} lines, cursor ({},{}), size {}x{}",
                        pane_id, lines.len(), cursor.0, cursor.1, size.cols, size.rows);

                    log::debug!("Pane {}: {} lines, cursor ({},{}), size {}x{}",
                        pane_id, lines.len(), cursor.0, cursor.1, size.cols, size.rows);

                    // Show some debug info in the UI
                    ui.label(format!("Pane {} | {} lines | {}x{} | cursor ({},{})",
                        pane_id, lines.len(), size.cols, size.rows, cursor.0, cursor.1));

                    let mut widget = TerminalWidget::with_content(lines, self.metrics.cell_width, self.metrics.cell_height);
                    widget = widget.cursor(cursor.0, cursor.1);
                    ui.add(widget);
                } else {
                    ui.label("No pane found");
                }
            } else {
                ui.label("No active pane");
            }
        });

        // Search overlay
        if self.app_state.search_open {
            match self.search.show(&egui.egui_ctx) {
                SearchAction::Close => self.app_state.search_open = false,
                _ => {}
            }
        }

        // Command palette
        if self.app_state.command_palette_open {
            if let Some(cmd) = self.command_palette.show(&egui.egui_ctx) {
                self.app_state.command_palette_open = false;
                match cmd.as_str() {
                    "New Tab" => spawn_new_tab = true,
                    "Close Tab" => close_tab = true,
                    "Search" => open_search = true,
                    _ => {}
                }
            }
        }

        // End egui frame
        let full_output = egui.egui_ctx.end_pass();
        egui.state.handle_platform_output(&window, full_output.platform_output);

        // Tessellate
        let paint_jobs = egui.egui_ctx.tessellate(full_output.shapes, egui.egui_ctx.pixels_per_point());

        // Update textures
        for (id, delta) in &full_output.textures_delta.set {
            egui.renderer.update_texture(device, queue, *id, delta);
        }

        // Update buffers
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui") });
        egui.renderer.update_buffers(device, queue, &mut encoder, &paint_jobs, &egui.screen_descriptor);

        // Get surface texture
        let output = surface.get_current_texture();
        let surface_texture = match output {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                log::warn!("Surface texture unavailable: {:?}", other);
                return;
            }
        };
        let view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Render egui
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.118, g: 0.118, b: 0.118, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }).forget_lifetime();
            egui.renderer.render(&mut rpass, &paint_jobs, &egui.screen_descriptor);
        }

        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        // Free textures
        for id in &full_output.textures_delta.free {
            egui.renderer.free_texture(id);
        }

        // Release borrow before deferred actions
        let _ = (egui, device, queue, surface, surface_config);

        // Deferred actions
        if spawn_new_tab { let _ = self.spawn_pane(); }
        if close_tab { self.send_input(b"exit\n".to_vec()); }
        if open_search { self.search = SearchOverlay::new(); self.app_state.search_open = true; }

        if let Some(w) = &self.window { w.request_redraw(); }
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
