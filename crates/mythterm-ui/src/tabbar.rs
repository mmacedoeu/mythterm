//! Tab bar widget.
//!
//! Displays a horizontal strip of tabs at the top of the window.
//! Styled for the cinematic dark theme: dark backgrounds, light text,
//! and a cyan accent bar on the active tab so the focus is obvious
//! at a glance.

use egui::{Response, Sense, Ui, Vec2, Widget};

use crate::color::srgb_to_display_color32;

/// Cinematic theme colors for the tab bar.
///
/// Each color is **pre-converted** from its sRGB-authored value through
/// `crate::color::srgb_to_display_color32`: the result is the `u8` that
/// the egui-wgpu renderer should write to the linear HDR target so
/// that, after the post-process ACES tonemap and the GPU's automatic
/// sRGB encoding on the swapchain write, the screen displays the
/// authored sRGB color. (See `crate::color` for the full pipeline and
/// the rationale.)
pub mod theme {
    use egui::Color32;

    /// Tab bar background (between tabs / under `+` button).
    /// sRGB `(20, 22, 28)` → ACES-aware.
    pub const BAR_BG: Color32 = Color32::from_rgb(4, 4, 5);
    /// Background of the active tab — slightly lifted from the bar.
    /// sRGB `(34, 38, 48)` → ACES-aware.
    pub const ACTIVE_BG: Color32 = Color32::from_rgb(7, 8, 10);
    /// Background of inactive tabs.
    /// sRGB `(24, 26, 32)` → ACES-aware.
    pub const INACTIVE_BG: Color32 = Color32::from_rgb(5, 5, 6);
    /// Text color on the active tab.
    /// sRGB `(232, 234, 240)` → ACES-aware.
    pub const ACTIVE_TEXT: Color32 = Color32::from_rgb(255, 255, 255);
    /// Text color on inactive tabs.
    /// sRGB `(140, 146, 158)` → ACES-aware.
    pub const INACTIVE_TEXT: Color32 = Color32::from_rgb(45, 49, 58);
    /// Cyan accent (focused-tab underline + close hover).
    /// sRGB `(72, 176, 224)` → ACES-aware.
    pub const ACCENT: Color32 = Color32::from_rgb(16, 76, 198);
    /// Hairline separator color.
    /// sRGB `(48, 52, 62)` → ACES-aware.
    pub const HAIRLINE: Color32 = Color32::from_rgb(10, 11, 13);
}

/// A tab bar widget that displays multiple tabs.
pub struct TabBar {
    /// Tab titles.
    titles: Vec<String>,
    /// Currently active tab index.
    active: usize,
    /// Height of the tab bar in pixels.
    height: f32,
}

impl TabBar {
    /// Create a new tab bar.
    pub fn new(titles: Vec<String>, active: usize) -> Self {
        Self {
            titles,
            active,
            height: 36.0,
        }
    }

    /// Set the tab bar height.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Show the tab bar and return the clicked tab index, if any.
    ///
    /// A returned index equal to `titles.len()` means the `+` button
    /// was clicked (request a new tab).
    pub fn show(&self, ui: &mut Ui) -> Option<usize> {
        let mut clicked = None;

        // Paint the bar background so the gap between tabs and the
        // strip below them doesn't show through to the swapchain.
        let bar_rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(bar_rect, 0.0, theme::BAR_BG);

        // Bottom hairline separator — gives the bar a defined edge
        // and matches the look in the goal mockup.
        let hairline_y = bar_rect.max.y - 0.5;
        ui.painter().line_segment(
            [
                egui::pos2(bar_rect.min.x, hairline_y),
                egui::pos2(bar_rect.max.x, hairline_y),
            ],
            egui::Stroke::new(1.0, theme::HAIRLINE),
        );

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;

            for (i, title) in self.titles.iter().enumerate() {
                let is_active = i == self.active;

                let bg_color = if is_active {
                    theme::ACTIVE_BG
                } else {
                    theme::INACTIVE_BG
                };
                let text_color = if is_active {
                    theme::ACTIVE_TEXT
                } else {
                    theme::INACTIVE_TEXT
                };

                let tab_width = 144.0;
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(tab_width, self.height),
                    Sense::click(),
                );

                if ui.is_rect_visible(rect) {
                    if is_active {
                        // Active tab: full premium treatment matching the
                        // goal mockup.
                        //
                        // Layered recipe (back to front):
                        //   1. Soft drop shadow behind the tab (elevation).
                        //   2. Rounded SDF rect with dark navy-to-black
                        //      vertical gradient fill.
                        //   3. Cyan emissive 1px border + multi-layer
                        //      bloom glow.
                        //   4. Inner top-edge highlight (specular).
                        //   5. Inner bottom-edge shadow.
                        //   6. Faint top gloss band (glass reflection).
                        let tab_cr = egui::CornerRadius::same(12);

                        // 1. Soft drop shadow.
                        let shadow_rect = rect.translate(egui::vec2(0.0, 3.0));
                        ui.painter().rect_filled(
                            shadow_rect,
                            tab_cr,
                            srgb_to_display_color32(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 55)),
                        );

                        // 2. Rounded gradient fill (navy -> near-black).
                        let top_c = srgb_to_display_color32(egui::Color32::from_rgb(22, 36, 55));
                        let bot_c = srgb_to_display_color32(egui::Color32::from_rgb(11, 16, 24));
                        let inset = 0.5_f32;
                        let fill_rect = rect.shrink(inset);
                        let fill_cr = egui::CornerRadius::same(11);
                        let mut mesh = egui::Mesh::default();
                        mesh.vertices.reserve(4);
                        mesh.indices.reserve(6);
                        let uv = egui::Pos2::ZERO;
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: fill_rect.left_top(),
                            color: top_c,
                            uv,
                        });
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: fill_rect.right_top(),
                            color: top_c,
                            uv,
                        });
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: fill_rect.left_bottom(),
                            color: bot_c,
                            uv,
                        });
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: fill_rect.right_bottom(),
                            color: bot_c,
                            uv,
                        });
                        mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
                        ui.painter().add(egui::Shape::mesh(mesh));

                        // 3. Cyan emissive border + multi-layer glow.
                        // Subtle in the goal — just a hint of cyan.
                        let border_rgb = egui::Color32::from_rgb(46, 167, 255);
                        let border_disp = srgb_to_display_color32(
                            egui::Color32::from_rgba_unmultiplied(46, 167, 255, 110),
                        );
                        ui.painter().rect_stroke(
                            fill_rect,
                            fill_cr,
                            egui::Stroke::new(1.0, border_disp),
                            egui::StrokeKind::Inside,
                        );
                        for &(w_mult, a_mult) in &[(2.0_f32, 0.18_f32), (4.5, 0.09), (8.0, 0.04)] {
                            let glow = egui::Color32::from_rgba_unmultiplied(
                                border_rgb.r(), border_rgb.g(), border_rgb.b(),
                                (a_mult * 255.0) as u8,
                            );
                            let glow_disp = srgb_to_display_color32(glow);
                            ui.painter().rect_stroke(
                                fill_rect,
                                fill_cr,
                                egui::Stroke::new(w_mult, glow_disp),
                                egui::StrokeKind::Inside,
                            );
                        }

                        // 4. Inner top-edge highlight (specular).
                        let hl = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22);
                        let hl_y = fill_rect.min.y + 0.5;
                        let hl_x_pad = 14.0_f32;
                        ui.painter().line_segment(
                            [
                                egui::pos2(fill_rect.min.x + hl_x_pad, hl_y),
                                egui::pos2(fill_rect.max.x - hl_x_pad, hl_y),
                            ],
                            egui::Stroke::new(1.0, hl),
                        );

                        // 5. Inner bottom-edge shadow.
                        let sh = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 65);
                        let sh_y = fill_rect.max.y - 0.5;
                        ui.painter().line_segment(
                            [
                                egui::pos2(fill_rect.min.x + hl_x_pad, sh_y),
                                egui::pos2(fill_rect.max.x - hl_x_pad, sh_y),
                            ],
                            egui::Stroke::new(1.0, sh),
                        );

                        // 6. Faint top gloss band (glass reflection).
                        let gloss_h = 6.0_f32;
                        let gloss_rect = egui::Rect::from_min_max(
                            egui::pos2(fill_rect.min.x + 8.0, fill_rect.min.y + 1.0),
                            egui::pos2(fill_rect.max.x - 8.0, fill_rect.min.y + 1.0 + gloss_h),
                        );
                        // Gradient mesh: white at top, transparent at bottom.
                        let mut gloss = egui::Mesh::default();
                        gloss.vertices.reserve(4);
                        gloss.indices.reserve(6);
                        let gloss_top = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28);
                        let gloss_bot = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 0);
                        gloss.vertices.push(egui::epaint::Vertex {
                            pos: gloss_rect.left_top(),
                            color: gloss_top,
                            uv,
                        });
                        gloss.vertices.push(egui::epaint::Vertex {
                            pos: gloss_rect.right_top(),
                            color: gloss_top,
                            uv,
                        });
                        gloss.vertices.push(egui::epaint::Vertex {
                            pos: gloss_rect.left_bottom(),
                            color: gloss_bot,
                            uv,
                        });
                        gloss.vertices.push(egui::epaint::Vertex {
                            pos: gloss_rect.right_bottom(),
                            color: gloss_bot,
                            uv,
                        });
                        gloss.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
                        ui.painter().add(egui::Shape::mesh(gloss));
                    } else {
                        ui.painter().rect_filled(rect, 0.0, bg_color);
                    }

                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        title,
                        egui::FontId::proportional(13.0),
                        text_color,
                    );
                }

                if response.clicked() {
                    clicked = Some(i);
                }
            }

            // "+" button for new tab.
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(self.height, self.height),
                Sense::click(),
            );
            if ui.is_rect_visible(rect) {
                ui.painter().rect_filled(rect, 0.0, theme::INACTIVE_BG);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(18.0),
                    theme::INACTIVE_TEXT,
                );
            }
            if response.clicked() {
                clicked = Some(self.titles.len());
            }
        });

        clicked
    }
}

impl Widget for TabBar {
    fn ui(self, ui: &mut Ui) -> Response {
        let _ = self.show(ui);
        ui.allocate_response(Vec2::ZERO, Sense::hover())
    }
}
