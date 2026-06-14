//! Tab bar widget.
//!
//! Displays a horizontal strip of tabs at the top of the window.
//! Styled for the cinematic dark theme: dark backgrounds, light text,
//! and a cyan accent bar on the active tab so the focus is obvious
//! at a glance.

use egui::{Response, Sense, Ui, Vec2, Widget};

use crate::color::srgb_to_display_color32;

/// Draw a horizontal line from x0 to x1 at y with an asymmetric
/// brightness gradient. `peak_t` is the relative position (0..1) of
/// the brightest point. `left_edge` is the RGB at x0, `right_edge` at
/// x1, `peak` is the RGB at the brightest point. Interpolates with
/// smoothstep. Colors are sent through srgb_to_display_color32 to bypass
/// post-process.
fn gradient_line(
    painter: &egui::Painter,
    x0: f32,
    x1: f32,
    y: f32,
    peak_t: f32,
    left_edge: (u8, u8, u8),
    peak: (u8, u8, u8),
    right_edge: (u8, u8, u8),
) {
    let steps = 20;
    let width = x1 - x0;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let xa = x0 + t0 * width;
        let xb = x0 + t1 * width;
        let mid_t = (t0 + t1) * 0.5;
        // Asymmetric falloff: distance from peak, normalized per side
        let (s, edge) = if mid_t < peak_t {
            let d = (peak_t - mid_t) / peak_t;
            (1.0 - d, left_edge)
        } else {
            let d = (mid_t - peak_t) / (1.0 - peak_t);
            (1.0 - d, right_edge)
        };
        let s = s.clamp(0.0, 1.0);
        let s = s * s * (3.0 - 2.0 * s); // smoothstep
        let r = edge.0 as f32 + (peak.0 as f32 - edge.0 as f32) * s;
        let g = edge.1 as f32 + (peak.1 as f32 - edge.1 as f32) * s;
        let b = edge.2 as f32 + (peak.2 as f32 - edge.2 as f32) * s;
        let color =
            srgb_to_display_color32(egui::Color32::from_rgb(r as u8, g as u8, b as u8));
        painter.line_segment(
            [egui::pos2(xa, y), egui::pos2(xb, y)],
            egui::Stroke::new(1.0, color),
        );
    }
}

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
                        // Active tab: per-border gradient pattern from
                        // the goal mockup, validated in isolated tests.
                        //
                        // The goal tab has:
                        //   - Sharp corners (no rounding)
                        //   - Top border: black gap + dim AA + bright peak
                        //     with horizontal brightness gradient (brighter
                        //     center-left, dimmer right edge) + transition
                        //   - Bottom border: ~7px from bottom, with dim
                        //     transition above peak, peak with same
                        //     horizontal gradient, anti-alias below, dark
                        //   - NO left/right borders (just fill)
                        //   - Asymmetric horizontal gradient on both
                        //     borders (right edge notably dimmer)
                        //
                        // All colors go through srgb_to_display_color32 to
                        // bypass the ACES tonemap + post-process so the
                        // authored sRGB values land on screen intact.
                        let cr = 0.0; // SHARP corners — goal has no rounding
                        let fill_c = srgb_to_display_color32(egui::Color32::from_rgb(22, 44, 62));
                        let painter = ui.painter();
                        painter.rect_filled(rect, cr, fill_c);

                        // ── Top border ──────────────────────────────
                        let top_y = rect.min.y;
                        // Row -2: black gap above
                        painter.line_segment(
                            [
                                egui::pos2(rect.min.x, top_y - 2.0),
                                egui::pos2(rect.max.x, top_y - 2.0),
                            ],
                            egui::Stroke::new(
                                1.0,
                                srgb_to_display_color32(egui::Color32::BLACK),
                            ),
                        );
                        // Row -1: dim cyan anti-alias
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            top_y - 1.0,
                            0.45,
                            (17, 64, 88),  // left edge
                            (22, 75, 98),  // peak
                            (15, 55, 72),  // right edge
                        );
                        // Row 0: bright cyan peak with horizontal gradient
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            top_y + 0.5,
                            0.45,
                            (37, 100, 136), // left edge
                            (45, 125, 165), // peak
                            (32, 93, 128),  // right edge
                        );
                        // Row +1: transition to fill
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            top_y + 1.5,
                            0.45,
                            (5, 26, 41),   // left edge
                            (8, 32, 50),   // peak
                            (5, 26, 42),   // right edge
                        );

                        // ── Bottom border ───────────────────────────
                        // Positioned ~4px from bottom (proportional to
                        // goal's ~7px in a 67px tab).
                        let bot_y = rect.max.y - 4.0;
                        // Row -1: dim transition above peak
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            bot_y - 1.0,
                            0.40,
                            (4, 25, 37),   // left edge
                            (6, 30, 45),   // peak
                            (4, 22, 35),   // right edge
                        );
                        // Row 0: bright cyan peak with horizontal gradient
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            bot_y + 0.0,
                            0.40,
                            (26, 111, 159), // left edge
                            (29, 138, 195), // peak
                            (27, 103, 141), // right edge
                        );
                        // Row +1: anti-alias below peak
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            bot_y + 1.0,
                            0.40,
                            (4, 42, 70),   // left edge
                            (8, 54, 82),   // peak
                            (6, 35, 57),   // right edge
                        );
                        // Row +2: dark area below
                        gradient_line(
                            painter,
                            rect.min.x,
                            rect.max.x,
                            bot_y + 2.0,
                            0.50,
                            (1, 3, 5),     // left edge
                            (2, 5, 8),     // peak
                            (0, 1, 3),     // right edge
                        );
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
