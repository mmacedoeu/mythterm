//! Isolated test binary for tab rendering.
//!
//! Strips out the full mythterm pipeline (no terminal, no mux, no
//! post-process, no bloom, no config) and renders ONLY a tab shape
//! directly to the swapchain. This lets us test each tab-rendering
//! layer one step at a time and observe what actually reaches the
//! screen.
//!
//! Usage:
//!   tab-test --step N   # N = 1..5
//!     1: solid colour rect_filled
//!     2: + rounded corners
//!     3: + vertical gradient mesh
//!     4: + cyan border
//!     5: + multi-layer bloom

use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Which rendering step to display.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Fill,
    Rounded,
    Gradient,
    Border,
    Glow,
}

impl Step {
    fn from_env() -> Self {
        let mut args = std::env::args().skip(1);
        match args.next().as_deref() {
            Some("--step") => match args.next().as_deref() {
                Some("1") => Step::Fill,
                Some("2") => Step::Rounded,
                Some("3") => Step::Gradient,
                Some("4") => Step::Border,
                Some("5") => Step::Glow,
                _ => Step::Fill,
            },
            _ => Step::Fill,
        }
    }
}

const GOAL_NAVY: egui::Color32 = egui::Color32::from_rgb(22, 44, 62);
const GOAL_BORDER: egui::Color32 = egui::Color32::from_rgb(72, 176, 224);

/// Draw a 300x60 tab for the given `step` at the top-left of `ui`.
fn draw_tab(ui: &mut egui::Ui, step: Step) {
    let (rect, _resp) = ui.allocate_exact_size(
        egui::vec2(300.0, 60.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let cr = egui::CornerRadius::same(12);

    match step {
        Step::Fill => {
            painter.rect_filled(rect, 0.0, GOAL_NAVY);
        }
        Step::Rounded => {
            painter.rect_filled(rect, 12.0, GOAL_NAVY);
        }
        Step::Gradient => {
            let top_c = egui::Color32::from_rgb(28, 50, 75);
            let bot_c = egui::Color32::from_rgb(14, 28, 42);
            let mut mesh = egui::Mesh::default();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_top(),
                color: top_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_top(),
                color: top_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_bottom(),
                color: bot_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_bottom(),
                color: bot_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
            painter.add(egui::Shape::mesh(mesh));
        }
        Step::Border => {
            let top_c = egui::Color32::from_rgb(28, 50, 75);
            let bot_c = egui::Color32::from_rgb(14, 28, 42);
            let mut mesh = egui::Mesh::default();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_top(),
                color: top_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_top(),
                color: top_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_bottom(),
                color: bot_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_bottom(),
                color: bot_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
            painter.add(egui::Shape::mesh(mesh));
            painter.rect_stroke(
                rect,
                cr,
                egui::Stroke::new(2.0, GOAL_BORDER),
                egui::StrokeKind::Inside,
            );
        }
        Step::Glow => {
            let top_c = egui::Color32::from_rgb(28, 50, 75);
            let bot_c = egui::Color32::from_rgb(14, 28, 42);
            let mut mesh = egui::Mesh::default();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_top(),
                color: top_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_top(),
                color: top_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_bottom(),
                color: bot_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_bottom(),
                color: bot_c,
                uv: egui::Pos2::ZERO,
            });
            mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
            painter.add(egui::Shape::mesh(mesh));
            painter.rect_stroke(
                rect,
                cr,
                egui::Stroke::new(2.0, GOAL_BORDER),
                egui::StrokeKind::Inside,
            );
            for &(w, a) in &[(4.0_f32, 0.35_f32), (8.0, 0.18), (14.0, 0.08)] {
                let glow = egui::Color32::from_rgba_unmultiplied(
                    GOAL_BORDER.r(), GOAL_BORDER.g(), GOAL_BORDER.b(),
                    (a * 255.0) as u8,
                );
                painter.rect_stroke(
                    rect,
                    cr,
                    egui::Stroke::new(w, glow),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

struct App {
    window: Option<Arc<Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    egui_ctx: Option<egui::Context>,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    screen_descriptor: Option<egui_wgpu::ScreenDescriptor>,
    step: Step,
}

impl App {
    fn new(step: Step) -> Self {
        Self {
            window: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            egui_ctx: None,
            egui_state: None,
            egui_renderer: None,
            screen_descriptor: None,
            step,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!("tab-test step={:?}", self.step))
                        .with_inner_size(winit::dpi::LogicalSize::new(1024, 768))
                        .with_decorations(false),
                )
                .expect("Failed to create window"),
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let (device, queue, surface_config) = pollster::block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .expect("Failed to find adapter");

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("tab-test"),
                        ..Default::default()
                    },
                )
                .await
                .expect("Failed to create device");

            let size = window.inner_size();
            let caps = surface.get_capabilities(&adapter);
            let format = caps.formats[0];
            let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
                wgpu::CompositeAlphaMode::PostMultiplied
            } else {
                caps.alpha_modes[0]
            };
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &config);
            (device, queue, config)
        });

        let egui_ctx = egui::Context::default();
        let viewport_id = egui_ctx.viewport_id();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            viewport_id,
            &window,
            None,
            None,
            None,
        );

        // CRITICAL: write to swapchain format directly (no HDR target).
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            surface_config.format,
            egui_wgpu::RendererOptions::default(),
        );

        self.screen_descriptor = Some(egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_config.width, surface_config.height],
            pixels_per_point: window.scale_factor() as f32,
        });
        self.window = Some(window);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.egui_ctx = Some(egui_ctx);
        self.egui_state = Some(egui_state);
        self.egui_renderer = Some(egui_renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(egui_state), Some(window)) = (&mut self.egui_state, &self.window) {
            let _ = egui_state.on_window_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.render();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn render(&mut self) {
        let step = self.step;
        let window = self.window.as_ref().unwrap();
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_ref().unwrap();
        let egui_ctx = self.egui_ctx.as_ref().unwrap();
        let egui_state = self.egui_state.as_mut().unwrap();
        let egui_renderer = self.egui_renderer.as_mut().unwrap();
        let screen_descriptor = self.screen_descriptor.as_ref().unwrap();

        let output = surface.get_current_texture();
        let surface_texture = match output {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                log::warn!("Surface texture unavailable: {:?}", other);
                return;
            }
        };
        let view = surface_texture.texture.create_view(&Default::default());

        // ---- egui frame ----
        let raw_input = egui_state.take_egui_input(window);
        let full_output = egui_ctx.run(raw_input, |ctx| {
            egui::Area::new(egui::Id::new("tab"))
                .fixed_pos(egui::pos2(20.0, 20.0))
                .show(ctx, |ui| {
                    draw_tab(ui, step);
                });
        });

        egui_state.handle_platform_output(window, full_output.platform_output);

        let tris = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, image_delta) in &full_output.textures_delta.set {
            egui_renderer.update_texture(device, queue, *id, image_delta);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        egui_renderer.update_buffers(
            device,
            queue,
            &mut encoder,
            &tris,
            screen_descriptor,
        );

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tab-test"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.08, g: 0.08, b: 0.09, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }).forget_lifetime();
            egui_renderer.render(&mut rpass, &tris, screen_descriptor);
        }

        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        for id in &full_output.textures_delta.free {
            egui_renderer.free_texture(id);
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let step = Step::from_env();
    log::info!("tab-test step: {:?}", step);
    let event_loop = EventLoop::new()?;
    let mut app = App::new(step);
    event_loop.run_app(&mut app)?;
    Ok(())
}
