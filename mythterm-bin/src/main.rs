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
use mythterm_mux::pane::PaneId;
use mythterm_mux::tab::Tab;
use mythterm_mux::Mux;
use mythterm_render::{BloomRenderer, LcdParams, PostProcess, RenderTarget, TonemapParams};
use mythterm_ui::input::InputMapper;
use mythterm_ui::overlay::{CommandPalette, SearchOverlay, SearchAction};
use mythterm_ui::srgb_to_display_color32;
use mythterm_ui::tabbar::TabBar;
use mythterm_ui::terminal_widget::TerminalWidget;
use mythterm_ui::AppState;

#[derive(Parser, Debug)]
#[command(name = "mythterm", about = "GPU-accelerated terminal emulator")]
struct Args {
    #[arg(long, short = 'n')]
    skip_config: bool,
}

/// Apply the cinematic dark theme to an egui context.
///
/// - Dark `panel_fill` and `window_fill` so panels (tab bar, etc.)
///   match the dark background instead of using egui's default
///   near-white surfaces.
/// - Cyan accent for `selection.bg_fill` so text selection and
///   interactive elements pick up the same accent as the active-tab
///   underline.
fn apply_cinematic_dark_theme(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(srgb_to_display_color32(egui::Color32::from_rgb(232, 234, 240)));
    visuals.panel_fill = srgb_to_display_color32(egui::Color32::from_rgb(20, 22, 28));
    visuals.window_fill = srgb_to_display_color32(egui::Color32::from_rgb(20, 22, 28));
    visuals.extreme_bg_color = srgb_to_display_color32(egui::Color32::from_rgb(10, 12, 16));
    visuals.faint_bg_color = srgb_to_display_color32(egui::Color32::from_rgb(28, 30, 36));
    visuals.selection.bg_fill = srgb_to_display_color32(egui::Color32::from_rgb(72, 176, 224));
    visuals.selection.stroke.color = srgb_to_display_color32(egui::Color32::from_rgb(232, 234, 240));
    visuals.hyperlink_color = srgb_to_display_color32(egui::Color32::from_rgb(72, 176, 224));
    ctx.set_visuals(visuals);
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
    render_target: Option<RenderTarget>,
    bloom: Option<BloomRenderer>,
    post_process: Option<PostProcess>,
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
            render_target: None,
            bloom: None,
            post_process: None,
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
        self.app_state.tab_titles.push(format!("Tab{}", tab_id + 1));
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
            .with_transparent(true)
            // Hide the OS title bar so the dark cinematic title bar
            // we draw in egui isn't fighting a light system title bar.
            .with_decorations(false)
            .with_resizable(true);
        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).expect("Failed to create surface");

        let (device, queue, surface_config, _format) = pollster::block_on(async {
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
        // Cinematic dark theme — without this, panels (tab bar, etc.)
        // use egui's default near-white `panel_fill` and the whole
        // chrome ends up light regardless of the terminal palette.
        apply_cinematic_dark_theme(&egui_ctx);

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

        let local_domain = LocalDomain::new(0, self.config.clone(), self.config.clone());

        let scale_factor = window.scale_factor();
        self.window = Some(window);

        // Create render target matching window size exactly
        let rt_width = surface_config.width;
        let rt_height = surface_config.height;
        let surface_format = surface_config.format;
        self.render_target = Some(RenderTarget::new(&device, rt_width, rt_height));
        // Bloom writes HDR (Rgba16Float) — never touches the swapchain.
        self.bloom = Some(BloomRenderer::new(&device, rt_width, rt_height));
        // PostProcess owns the two HDR scene textures (bloom→LCD→
        // tonemap chain) and writes the final sRGB-ready result to
        // the swapchain.
        let post = PostProcess::new(&device, &queue, surface_format, rt_width, rt_height);
        // Apply LCD subpixel + tonemap pass parameters from config.
        {
            let s = self.config.get_settings();
            let cinematic = &s.cinematic;
            use mythterm_render::{CurvatureParams, GlassParams};
            // Curvature is only used by the post-process vertex shader
            // (`vs_main`); bloom uses its own passthrough vertex shader
            // (`bloom_vs_main`) since bloom writes to flat intermediates.
            let curvature = CurvatureParams {
                strength: cinematic.screen_curvature,
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
            };
            post.set_curvature_params(&queue, curvature);
            post.set_glass_params(
                &queue,
                GlassParams {
                    intensity: cinematic.glass_intensity,
                    fresnel_bias: cinematic.glass_fresnel_bias,
                    top_falloff: cinematic.glass_top_falloff,
                    _pad0: 0.0,
                    ceiling_color: [
                        cinematic.glass_ceiling_color[0],
                        cinematic.glass_ceiling_color[1],
                        cinematic.glass_ceiling_color[2],
                        1.0,
                    ],
                },
            );
            post.set_lcd_params(&queue, LcdParams {
                strength: s.cinematic.lcd_strength,
                subpixel_width: s.cinematic.lcd_subpixel_width,
                scanline: s.cinematic.lcd_scanline,
                _pad: 0.0,
            });
            let ec = s.cinematic.edge_color;
            post.set_tonemap_params(&queue, TonemapParams {
                micro_contrast: s.cinematic.micro_contrast,
                vignette: s.cinematic.vignette,
                edge_intensity: s.cinematic.edge_intensity,
                edge_width: s.cinematic.edge_width,
                edge_color: [ec[0], ec[1], ec[2], 0.0],
            });
        }
        self.post_process = Some(post);

        // Create egui renderer for the render target format (HDR)
        let egui_renderer = egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba16Float, egui_wgpu::RendererOptions::default());

        // Screen descriptor matches render target size
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [rt_width, rt_height],
            pixels_per_point: scale_factor as f32,
        };

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
                // Update render target to match new window size
                if let (Some(device), Some(rt)) = (&self.device, &mut self.render_target) {
                    rt.resize(device, new_size.width.max(1), new_size.height.max(1));
                }
                // Update bloom mip chain to match new window size
                if let (Some(device), Some(bloom)) = (&self.device, &mut self.bloom) {
                    bloom.resize(device, new_size.width.max(1), new_size.height.max(1));
                }
                // Update post-process scene texture
                if let (Some(device), Some(pp)) = (&self.device, &mut self.post_process) {
                    pp.resize(device, new_size.width.max(1), new_size.height.max(1));
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.should_quit {
            event_loop.exit();
            return;
        }
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
        let mut tab_clicked = false;

        // Custom title bar (cinematic, dark).
        //
        // We hide the OS title bar via `with_decorations(false)` and
        // draw our own so the chrome stays dark even when the user's
        // system theme is light. The bar is also the window-drag
        // region — egui's `Sense::drag()` plus `Window::drag_window()`
        // hands the gesture off to the WM.
        //
        // We bundle the title bar AND the tab bar into a single
        // top panel so they share one `available_rect` and stack
        // correctly. Stacking two `Panel::top` calls in egui 0.34
        // leaves a gap between them, which is exactly what the
        // previous "two-panel" layout produced.
        let title_bar_h = 28.0;
        let tab_bar_h = 36.0;
        let chrome_h = title_bar_h + tab_bar_h;
        let settings = self.config.get_settings();
        let corner_radius_u8 = settings.cinematic.window_corner_radius as u8;

        // Window-level rounded background + bright border glow.
        //
        // Drawn on the background layer so it sits BEHIND the chrome
        // and central panels. The chrome panel's fill is set to match
        // this background so the chrome is a smooth continuation of
        // the window. The central panel has its own fill; its rounded
        // bottom corners leave small gaps that show this background
        // through, which reads as a subtle "card" vignette at the
        // corners.
        let chrome_bg = srgb_to_display_color32(egui::Color32::from_rgb(20, 22, 28));
        // The full window area (including OS title bar) — used for the
        // background fill so the rounded corners match the window shape.
        let screen_rect = egui.egui_ctx.screen_rect();
        let bg_painter = egui.egui_ctx.layer_painter(egui::LayerId::background());
        bg_painter.rect_filled(
            screen_rect,
            egui::CornerRadius::same(corner_radius_u8),
            chrome_bg,
        );
        // (The bright border glow is drawn at the END of the frame on a
        // foreground layer so it sits ON TOP of the chrome and central
        // panels rather than being covered by them. See below.)

        #[allow(deprecated)]
        egui::Panel::top("chrome")
            .frame(egui::Frame {
                inner_margin: egui::Margin::ZERO,
                fill: egui::Color32::TRANSPARENT,
                stroke: egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                ..Default::default()
            })
            .show_separator_line(false)
            .exact_height(chrome_h)
            .show(&egui.egui_ctx, |ui| {
                // Draw the chrome panel's background ourselves (the Frame's
                // paint uses content_ui.min_rect() which can be smaller than
                // the panel rect, causing a y-offset bug in egui 0.34.x).
                let chrome_panel_rect = ui.max_rect();
                ui.painter().rect_filled(
                    chrome_panel_rect,
                    egui::CornerRadius::ZERO,
                    chrome_bg,
                );

                let bar_rect = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(ui.available_width(), title_bar_h),
                );
                let button_w = 36.0;

                // Allocate the drag area on the left. This advances
                // the cursor past the title-bar height.
                let drag_rect = egui::Rect::from_min_max(
                    bar_rect.min,
                    egui::pos2(bar_rect.max.x - 3.0 * button_w, bar_rect.max.y),
                );
                let drag_response = ui.allocate_exact_size(drag_rect.size(), egui::Sense::drag()).1;

                if drag_response.drag_started() {
                    if let Some(w) = &self.window {
                        if let Err(e) = w.drag_window() {
                            log::debug!("drag_window: {e:?}");
                        }
                    }
                }

                // Centered title.
                ui.painter().text(
                    drag_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "mythterm",
                    egui::FontId::proportional(13.0),
                    srgb_to_display_color32(egui::Color32::from_rgb(232, 234, 240)),
                );

                // Window controls (right side).
                //
                // We use `ui.interact` (not `allocate_exact_size`)
                // so the cursor doesn't advance vertically for the
                // three buttons — they live inside the same row as
                // the drag area, and advancing the cursor would
                // push the tab bar out of the panel.
                let btn = |ui: &mut egui::Ui, x: f32, label: &str, fg: egui::Color32, hover: egui::Color32| -> bool {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, bar_rect.min.y),
                        egui::vec2(button_w, title_bar_h),
                    );
                    let r = ui.interact(rect, ui.id().with(("win_btn", label)), egui::Sense::click());
                    if r.hovered() {
                        ui.painter().rect_filled(rect, 0.0, hover);
                    }
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(14.0),
                        fg,
                    );
                    r.clicked()
                };
                let close_hover = srgb_to_display_color32(egui::Color32::from_rgb(232, 76, 76));
                let base_fg = srgb_to_display_color32(egui::Color32::from_rgb(180, 184, 196));
                let neutral_hover = srgb_to_display_color32(egui::Color32::from_rgb(58, 62, 74));
                let btn_x = bar_rect.max.x - 3.0 * button_w;
                if btn(ui, btn_x, "\u{2014}", base_fg, neutral_hover) {
                    if let Some(w) = &self.window { w.set_minimized(true); }
                }
                if btn(ui, btn_x + button_w, "\u{25A1}", base_fg, neutral_hover) {
                    if let Some(w) = &self.window {
                        w.set_maximized(!w.is_maximized());
                    }
                }
                if btn(ui, btn_x + 2.0 * button_w, "\u{2715}", base_fg, close_hover) {
                    self.should_quit = true;
                }

                // Tab bar (same `chrome` panel). The cursor is
                // already at y=title_bar_h thanks to the drag-area
                // allocation above, so the tab bar naturally
                // stacks directly below the title bar.
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

        // Terminal content.
        //
        // We *cannot* use `Frame::NONE` here because the window is
        // created with `with_transparent(true)` so the corner
        // radius / cinematic look can punch through. A transparent
        // central-panel frame therefore lets the desktop show
        // through any rows the terminal widget doesn't cover (a
        // bare prompt is only 1 row tall). The terminal widget
        // itself draws a background rect, but for safety we also
        // fill the central panel with the configured background
        // color — this way the entire region is dark even if the
        // widget is smaller than expected.
        // The egui-wgpu renderer writes vertex colors as-is to our
        // linear HDR target, which is then ACES-tonemapped and
        // sRGB-encoded on the swapchain write. Config colors are
        // sRGB-authored and would otherwise come out brightened
        // (e.g. #1E1E1E -> #6F6F6F). `srgb_to_display_color32`
        // inverts the full chain (sRGB decode -> ACES inverse) so
        // the screen displays the authored sRGB value.
        let panel_bg = srgb_to_display_color32(egui::Color32::from_rgb(
            settings.color_scheme.background[0],
            settings.color_scheme.background[1],
            settings.color_scheme.background[2],
        ));
        #[allow(deprecated)]
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: panel_bg,
                stroke: egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                corner_radius: egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: corner_radius_u8,
                    se: corner_radius_u8,
                },
                ..egui::Frame::default()
            })
            .show(&egui.egui_ctx, |ui| {
            // "Light from the right" gradient overlay: a subtle horizontal
            // gradient that brightens and blue-tints the right side of the
            // terminal background, matching the goal mockup. Drawn as a
            // mesh with per-vertex colors so it sits on top of the panel
            // fill but below the terminal text.
            if settings.cinematic.window_light_from_right
                && settings.cinematic.window_light_from_right_strength > 0.0
            {
                let panel_rect = ui.max_rect();
                let strength = settings.cinematic.window_light_from_right_strength.clamp(0.0, 1.0);
                // Work in authored sRGB space (0..255) so the gradient is
                // authored directly, then convert to display space at the end.
                let bg_r = settings.color_scheme.background[0] as f32;
                let bg_g = settings.color_scheme.background[1] as f32;
                let bg_b = settings.color_scheme.background[2] as f32;
                let glow_r = settings.cinematic.window_border_glow[0] as f32;
                let glow_g = settings.cinematic.window_border_glow[1] as f32;
                let glow_b = settings.cinematic.window_border_glow[2] as f32;
                // Right side: blend background with the border glow color
                // (additive bias toward cyan/blue) and a small extra
                // brightness boost so the right side reads as "lit".
                let blend = strength * 0.18;
                let boost = 14.0 * strength;
                let right_r = (bg_r * (1.0 - blend) + glow_r * blend).clamp(0.0, 255.0);
                let right_g = (bg_g * (1.0 - blend) + glow_g * blend + boost * 0.4).clamp(0.0, 255.0);
                let right_b = (bg_b * (1.0 - blend) + glow_b * blend + boost).clamp(0.0, 255.0);
                let right_c = srgb_to_display_color32(egui::Color32::from_rgb(
                    right_r as u8, right_g as u8, right_b as u8,
                ));
                let left_disp = srgb_to_display_color32(egui::Color32::from_rgb(
                    bg_r as u8, bg_g as u8, bg_b as u8,
                ));

                let mut mesh = egui::Mesh::default();
                mesh.vertices.reserve(4);
                mesh.indices.reserve(6);
                let uv = egui::Pos2::ZERO;
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: panel_rect.left_top(),
                    color: left_disp,
                    uv,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: panel_rect.right_top(),
                    color: right_c,
                    uv,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: panel_rect.left_bottom(),
                    color: left_disp,
                    uv,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: panel_rect.right_bottom(),
                    color: right_c,
                    uv,
                });
                mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
                ui.painter().add(egui::Shape::mesh(mesh));
            }

            // Border glow drawn inside the central panel, after the
            // gradient mesh, so it's guaranteed to be on top of both
            // the panel fill and the gradient. The rect is offset
            // by the OS title-bar height so the top border isn't
            // hidden behind the title bar.
            if settings.cinematic.window_border_glow_width > 0.0 {
                let base = egui::Color32::from_rgb(
                    settings.cinematic.window_border_glow[0],
                    settings.cinematic.window_border_glow[1],
                    settings.cinematic.window_border_glow[2],
                );
                let base_disp = srgb_to_display_color32(base);
                let title_bar_h = 12.0_f32;
                let border_rect = egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().min.x, ui.max_rect().min.y + title_bar_h),
                    ui.max_rect().max,
                );
                let cr = egui::CornerRadius::same(corner_radius_u8);
                // Inner crisp stroke.
                ui.painter().rect_stroke(
                    border_rect,
                    cr,
                    egui::Stroke::new(settings.cinematic.window_border_glow_width, base_disp),
                    egui::StrokeKind::Inside,
                );
                // Outer glow layers.
                for (w_mult, a_mult) in [(2.2, 0.85_f32), (3.8, 0.50), (6.0, 0.22)].iter() {
                    let glow = egui::Color32::from_rgba_unmultiplied(
                        base.r(), base.g(), base.b(), (a_mult * 255.0) as u8,
                    );
                    let glow_disp = srgb_to_display_color32(glow);
                    ui.painter().rect_stroke(
                        border_rect,
                        cr,
                        egui::Stroke::new(settings.cinematic.window_border_glow_width * w_mult, glow_disp),
                        egui::StrokeKind::Inside,
                    );
                }
            }

            if let Some(pane_id) = self.active_pane {
                if let Some(pane) = self.mux.get_pane(pane_id) {
                    let colored_lines = pane.get_colored_lines();
                    let cursor = pane.get_cursor_position();

                    // When the "light from the right" gradient is
                    // enabled, the terminal widget's own opaque background
                    // would cover the gradient mesh, so we force the widget
                    // to render with a transparent background and let the
                    // gradient (drawn earlier in this callback) show through.
                    let widget_bg_opacity = if settings.cinematic.window_light_from_right
                        && settings.cinematic.window_light_from_right_strength > 0.0
                    {
                        0.0
                    } else {
                        self.bg_opacity
                    };
                    let widget = TerminalWidget::with_colored_content(
                        colored_lines,
                        self.metrics.cell_width,
                        self.metrics.cell_height,
                    )
                    .bg_color(panel_bg)
                    .cursor(cursor.0, cursor.1)
                    .bg_opacity(widget_bg_opacity);
                    ui.add(widget);
                }
            }
        });

        // Window border glow is now drawn inside the central panel's
        // show callback (after the gradient mesh) so it's guaranteed
        // to be on top of the panel fill and gradient.
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

        // Render egui into offscreen render target (HDR texture)
        if let Some(rt) = &self.render_target {
            // Use the configured background color in linear space.
            // The render target is `Rgba16Float` (linear), and the
            // tonemap pass writes to an sRGB swapchain, so a
            // direct `0.118` value here would be gamma-encoded on
            // output and display as ~#646464 instead of the
            // configured #1E1E1E.
            let [lr, lg, lb] = self.config.get_settings().color_scheme.background_linear();
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui -> render target"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &rt.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: lr as f64,
                            g: lg as f64,
                            b: lb as f64,
                            a: 1.0,
                        }),
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

        // Apply bloom: read from the egui render target, write the
        // combined HDR result to the post-process scene buffer. Bloom
        // never touches the swapchain.
        if let (Some(rt), Some(bloom), Some(pp)) = (&self.render_target, &self.bloom, &self.post_process) {
            bloom.render(
                device,
                &mut encoder,
                &rt.sample_view,
                &rt.sampler,
                pp.scene_view(),
            );
        }

        // Tonemap the HDR scene (bloom output) to the swapchain.
        if let Some(pp) = &self.post_process {
            pp.render(device, &mut encoder, &view);
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
