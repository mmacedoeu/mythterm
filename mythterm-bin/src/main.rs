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
use mythterm_mux::Mux;
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
    bg_opacity: f32,
    should_quit: bool,
}

impl MythtermApp {
    fn new(args: Args) -> Result<Self> {
        let settings = if args.skip_config {
            Settings::default()
        } else {
            ensure_config_exists()?;
            load_settings()?
        };

        let bg_opacity = settings.background_opacity;

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
            bg_opacity,
            should_quit: false,
        })
    }

    fn spawn_pane(&mut self) -> Result<PaneId> {
        let domain = self.local_domain.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Local domain not initialized"))?;

        // Calculate terminal size from current window
        let size = if let Some(window) = &self.window {
            let ws = window.inner_size();
            let scale = window.scale_factor() as f32;
            let tab_bar_height = 32.0 * scale;
            let cols = ((ws.width as f32) / (self.metrics.cell_width * scale)).max(1.0) as u16;
            let rows = (((ws.height as f32) - tab_bar_height) / (self.metrics.cell_height * scale)).max(1.0) as u16;
            portable_pty::PtySize {
                rows,
                cols,
                pixel_width: ws.width as u16,
                pixel_height: ws.height as u16,
            }
        } else {
            portable_pty::PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }
        };

        let pane_id = self.mux.alloc_pane_id();
        let pane = domain.spawn(pane_id, size, None)?;
        let tab_id = self.mux.alloc_tab_id();
        self.mux.insert_pane(pane);
        self.mux.insert_tab(Arc::new(Tab::new(tab_id, self.mux.get_pane(pane_id).unwrap())));
        self.active_pane = Some(pane_id);
        self.app_state.tab_titles.push(format!("Tab {}", tab_id + 1));
        self.app_state.tab_pane_ids.push(pane_id);
        self.app_state.active_tab = self.app_state.tab_titles.len() - 1;
        log::info!("Spawned pane {} in tab {}", pane_id, tab_id);
        Ok(pane_id)
    }

    /// Sync active_pane with the active_tab index.
    fn sync_active_pane(&mut self) {
        if let Some(&pane_id) = self.app_state.tab_pane_ids.get(self.app_state.active_tab) {
            self.active_pane = Some(pane_id);
        }
    }

    /// Remove dead panes and their tabs. Returns true if any were removed.
    fn cleanup_dead_panes(&mut self) -> bool {
        let mut removed = false;
        let pane_ids: Vec<_> = self.mux.iter_panes().iter()
            .filter(|p| p.is_dead())
            .map(|p| p.pane_id())
            .collect();

        for pane_id in pane_ids {
            log::info!("Pane {} exited, removing", pane_id);
            self.mux.remove_pane(pane_id);

            // Remove from tab tracking
            if let Some(pos) = self.app_state.tab_pane_ids.iter().position(|&id| id == pane_id) {
                self.app_state.tab_pane_ids.remove(pos);
                if pos < self.app_state.tab_titles.len() {
                    self.app_state.tab_titles.remove(pos);
                }
                // Also remove the tab from mux
                let tabs: Vec<_> = self.mux.iter_tabs();
                for tab in &tabs {
                    if tab.panes().iter().any(|p| p.pane_id() == pane_id) {
                        self.mux.remove_tab(tab.tab_id());
                        break;
                    }
                }
                removed = true;
            }
        }

        // If current pane was removed, switch to first available
        if removed {
            if self.active_pane.map_or(false, |id| self.mux.get_pane(id).is_none()) {
                self.active_pane = self.app_state.tab_pane_ids.first().copied();
                self.app_state.active_tab = 0;
            }
        }

        removed
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
            .with_inner_size(winit::dpi::LogicalSize::new(1024, 768))
            .with_transparent(true);
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

            // Prefer an alpha mode that supports transparency
            let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
                wgpu::CompositeAlphaMode::PostMultiplied
            } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Inherit) {
                wgpu::CompositeAlphaMode::Inherit
            } else {
                caps.alpha_modes[0]
            };

            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &surface_config);

            (device, queue, surface_config, format)
        });

        let egui_ctx = Context::default();

        // Load Nerd Font for terminal rendering
        let font_discovery = mythterm_font::FontDiscovery::new();
        let mut font_loaded = false;

        // Try to load a Nerd Font
        for family in &[
            "JetBrainsMono Nerd Font",
            "FiraCode Nerd Font",
            "Hack Nerd Font",
            "Iosevka Nerd Font",
            "Cascadia Code",
            "monospace",
        ] {
            if let Ok(font_data) = font_discovery.find_font(family, false, false) {
                log::info!("Loaded font: {}", family);
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    "terminal_font".to_owned(),
                    Arc::new(egui::FontData::from_owned(font_data.data)),
                );
                // Set as the default monospace font
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "terminal_font".to_owned());
                // Also add to proportional as fallback
                fonts.families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("terminal_font".to_owned());
                egui_ctx.set_fonts(fonts);
                font_loaded = true;
                break;
            }
        }

        // Log available fallback fonts (Nerd Font symbols, emoji)
        let fallbacks = font_discovery.find_fallback_fonts();
        for fallback in &fallbacks {
            log::info!("Found fallback font: {} ({} bytes)", fallback.family, fallback.data.len());
        }

        if !font_loaded {
            log::warn!("No custom font loaded, using egui default");
        }

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
        if self.should_quit {
            event_loop.exit();
            return;
        }
        if let (Some(egui), Some(window)) = (&mut self.egui, &self.window) {
            let _ = egui.state.on_window_event(window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                // Update wgpu surface
                if let (Some(device), Some(surface), Some(config)) = (&self.device, &self.surface, &mut self.surface_config) {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, config);
                }
                // Update egui screen descriptor
                if let Some(egui) = &mut self.egui {
                    egui.screen_descriptor.size_in_pixels = [new_size.width, new_size.height];
                }
                // Calculate new terminal size in cells
                // Account for tab bar height (32px) and scale factor
                let scale = self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0);
                let tab_bar_height = 32.0 * scale;
                let avail_width = new_size.width as f32;
                let avail_height = (new_size.height as f32 - tab_bar_height).max(1.0);
                let cols = (avail_width / (self.metrics.cell_width * scale)).max(1.0) as u16;
                let rows = (avail_height / (self.metrics.cell_height * scale)).max(1.0) as u16;

                log::debug!("Resize: {}x{} -> {}cols x {}rows", new_size.width, new_size.height, cols, rows);

                // Notify active pane of new size
                if let Some(pane_id) = self.active_pane {
                    if let Some(pane) = self.mux.get_pane(pane_id) {
                        let new_size = portable_pty::PtySize {
                            rows,
                            cols,
                            pixel_width: new_size.width as u16,
                            pixel_height: new_size.height as u16,
                        };
                        if let Err(e) = pane.resize(new_size) {
                            log::error!("Failed to resize pane {}: {}", pane_id, e);
                        }
                    }
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
                    } else if let Some(t) = text {
                        // Printable characters (space, symbols, etc.)
                        self.send_input(t.as_bytes().to_vec());
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
        let mut tab_clicked = false;
        egui::TopBottomPanel::top("tab_bar").show(&egui.egui_ctx, |ui| {
            let tab_bar = TabBar::new(self.app_state.tab_titles.clone(), self.app_state.active_tab);
            if let Some(clicked) = tab_bar.show(ui) {
                if clicked < self.app_state.tab_titles.len() {
                    self.app_state.active_tab = clicked;
                    tab_clicked = true;
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

                    let mut widget = TerminalWidget::with_content(lines, self.metrics.cell_width, self.metrics.cell_height);
                    widget = widget.cursor(cursor.0, cursor.1).bg_opacity(self.bg_opacity);
                    ui.add(widget);
                }
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
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.118, g: 0.118, b: 0.118, a: self.bg_opacity as f64 }),
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
        if tab_clicked { self.sync_active_pane(); }
        if close_tab { self.send_input(b"exit\n".to_vec()); }
        if open_search { self.search = SearchOverlay::new(); self.app_state.search_open = true; }

        // Cleanup dead panes (shell exited)
        self.cleanup_dead_panes();

        // Quit if no panes left
        if self.mux.iter_panes().is_empty() {
            self.should_quit = true;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }

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
