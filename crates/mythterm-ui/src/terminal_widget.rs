//! Terminal widget: egui widget that renders terminal content.

use crate::color::srgb_to_display_color32;
use egui::{Color32, FontId, Rect, Response, Sense, Ui, Vec2, Widget};

/// Cursor style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    /// Solid block cursor.
    Block,
    /// Vertical beam cursor.
    Beam,
    /// Underline cursor.
    Underline,
}

/// A widget that displays terminal content.
pub struct TerminalWidget {
    /// Lines of text to display (plain text, no colors).
    lines: Vec<String>,
    /// Colored lines (text + per-character colors).
    colored_lines: Vec<(String, Vec<([u8; 3], [u8; 3])>)>,
    /// Whether to use colored_lines instead of lines.
    use_colors: bool,
    /// Width in cells.
    cols: usize,
    /// Height in cells.
    rows: usize,
    /// Cell width in pixels.
    cell_width: f32,
    /// Cell height in pixels.
    cell_height: f32,
    /// Background color.
    bg_color: Color32,
    /// Foreground color.
    fg_color: Color32,
    /// Cursor position (col, row).
    cursor: Option<(usize, usize)>,
    /// Cursor style.
    cursor_style: CursorStyle,
    /// Background opacity (0.0 - 1.0).
    bg_opacity: f32,
    /// Cursor blink rate in milliseconds (0 = no blink).
    cursor_blink_ms: u64,
    /// Cursor color.
    cursor_color: Color32,
}

impl TerminalWidget {
    /// Create a new terminal widget with empty content.
    pub fn new(cols: usize, rows: usize, cell_width: f32, cell_height: f32) -> Self {
        Self {
            lines: vec![String::new(); rows],
            colored_lines: Vec::new(),
            use_colors: false,
            cols,
            rows,
            cell_width,
            cell_height,
            bg_color: srgb_to_display_color32(Color32::from_rgb(30, 30, 30)),
            fg_color: srgb_to_display_color32(Color32::from_rgb(192, 192, 192)),
            cursor: Some((0, 0)),
            cursor_style: CursorStyle::Block,
            bg_opacity: 1.0,
            cursor_blink_ms: 500,
            cursor_color: srgb_to_display_color32(Color32::from_rgb(200, 200, 200)),
        }
    }

    /// Create a terminal widget with plain text content.
    pub fn with_content(lines: Vec<String>, cell_width: f32, cell_height: f32) -> Self {
        let rows = lines.len().max(1);
        let cols = lines.iter().map(|l| l.len()).max().unwrap_or(80);
        Self {
            lines,
            colored_lines: Vec::new(),
            use_colors: false,
            cols,
            rows,
            cell_width,
            cell_height,
            bg_color: srgb_to_display_color32(Color32::from_rgb(30, 30, 30)),
            fg_color: srgb_to_display_color32(Color32::from_rgb(192, 192, 192)),
            cursor: Some((0, 0)),
            cursor_style: CursorStyle::Block,
            bg_opacity: 1.0,
            cursor_blink_ms: 500,
            cursor_color: srgb_to_display_color32(Color32::from_rgb(200, 200, 200)),
        }
    }

    /// Create a terminal widget with colored content.
    pub fn with_colored_content(
        colored_lines: Vec<(String, Vec<([u8; 3], [u8; 3])>)>,
        cell_width: f32,
        cell_height: f32,
    ) -> Self {
        let rows = colored_lines.len().max(1);
        let cols = colored_lines.iter().map(|(l, _)| l.len()).max().unwrap_or(80);
        let lines = colored_lines.iter().map(|(l, _)| l.clone()).collect();
        Self {
            lines,
            colored_lines,
            use_colors: true,
            cols,
            rows,
            cell_width,
            cell_height,
            bg_color: srgb_to_display_color32(Color32::from_rgb(30, 30, 30)),
            fg_color: srgb_to_display_color32(Color32::from_rgb(192, 192, 192)),
            cursor: Some((0, 0)),
            cursor_style: CursorStyle::Block,
            bg_opacity: 1.0,
            cursor_blink_ms: 500,
            cursor_color: srgb_to_display_color32(Color32::from_rgb(200, 200, 200)),
        }
    }

    /// Set the background color.
    pub fn bg_color(mut self, color: Color32) -> Self {
        self.bg_color = color;
        self
    }

    /// Set the foreground color.
    pub fn fg_color(mut self, color: Color32) -> Self {
        self.fg_color = color;
        self
    }

    /// Set the cursor position.
    pub fn cursor(mut self, col: usize, row: usize) -> Self {
        self.cursor = Some((col, row));
        self
    }

    /// Set the cursor style.
    pub fn cursor_style(mut self, style: CursorStyle) -> Self {
        self.cursor_style = style;
        self
    }

    /// Set background opacity (0.0 = transparent, 1.0 = opaque).
    pub fn bg_opacity(mut self, opacity: f32) -> Self {
        self.bg_opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Set cursor blink rate in milliseconds (0 = no blink).
    pub fn cursor_blink_ms(mut self, ms: u64) -> Self {
        self.cursor_blink_ms = ms;
        self
    }

    /// Set cursor color.
    pub fn cursor_color(mut self, color: Color32) -> Self {
        self.cursor_color = color;
        self
    }

    /// Update the content from terminal lines.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
    }
}

impl Widget for TerminalWidget {
    fn ui(mut self, ui: &mut Ui) -> Response {
        // Fill the available area (rounded down to whole cells) so
        // the background covers the whole central panel, not just
        // however many rows/cols the current shell content happens
        // to have. Without this, a bare prompt would leave a sea of
        // transparency — and with `with_transparent(true)`, that
        // transparency shows the desktop, not our dark background.
        let avail = ui.available_size();
        let fill_rows = (avail.y / self.cell_height).floor() as usize;
        let fill_cols = (avail.x / self.cell_width).floor() as usize;
        if self.rows < fill_rows {
            self.rows = fill_rows;
        }
        if self.cols < fill_cols {
            self.cols = fill_cols;
        }

        let desired_size = Vec2::new(
            self.cols as f32 * self.cell_width,
            self.rows as f32 * self.cell_height,
        );

        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Draw background with opacity
            let [r, g, b, _] = self.bg_color.to_array();
            let a = (self.bg_opacity * 255.0) as u8;
            let bg = Color32::from_rgba_premultiplied(r, g, b, a);
            painter.rect_filled(rect, 0.0, bg);

            // Draw text lines and cursor
            let font_id = FontId::monospace(self.cell_height * 0.8);

            for (row, line) in self.lines.iter().enumerate() {
                let y = rect.min.y + row as f32 * self.cell_height;

                if self.use_colors {
                    // Render with per-character colors
                    if let Some((_, colors)) = self.colored_lines.get(row) {
                        let mut x = rect.min.x;
                        for (ch, (fg_rgb, bg_rgb)) in line.chars().zip(colors.iter()) {
                            let ch_str = ch.to_string();
                            let ch_width = painter.layout_no_wrap(
                                ch_str.clone(),
                                font_id.clone(),
                                Color32::TRANSPARENT,
                            ).size().x;

                            // Draw character background if not default
                            if *bg_rgb != [30, 30, 30] {
                                let bg_rect = Rect::from_min_size(
                                    egui::pos2(x, y),
                                    Vec2::new(ch_width, self.cell_height),
                                );
                                let [r, g, b] = bg_rgb;
                                let a = (self.bg_opacity * 255.0) as u8;
                                // The terminal gives us raw sRGB color tuples
                                // for each cell; convert to the linear-HDR
                                // representation egui will write to the target.
                                let bg = srgb_to_display_color32(Color32::from_rgba_unmultiplied(*r, *g, *b, a));
                                painter.rect_filled(bg_rect, 0.0, Color32::from_rgba_premultiplied(bg.r(), bg.g(), bg.b(), bg.a()));
                            }

                            // Draw character
                            let [r, g, b] = fg_rgb;
                            let fg = srgb_to_display_color32(Color32::from_rgb(*r, *g, *b));
                            painter.text(
                                egui::pos2(x, y),
                                egui::Align2::LEFT_TOP,
                                &ch_str,
                                font_id.clone(),
                                fg,
                            );

                            x += ch_width;
                        }
                    }
                } else {
                    // Render with default colors
                    let galley = painter.layout_no_wrap(
                        line.clone(),
                        font_id.clone(),
                        self.fg_color,
                    );
                    painter.galley(egui::pos2(rect.min.x, y), galley, self.fg_color);
                }

                // Draw cursor on this row
                if let Some((col, row_idx)) = self.cursor {
                    if row_idx == row && row < self.rows {
                        let show_cursor = if self.cursor_blink_ms > 0 {
                            let time_secs = ui.input(|i| i.time);
                            let blink_secs = self.cursor_blink_ms as f64 / 1000.0;
                            let phase = (time_secs / blink_secs) as u64;
                            phase % 2 == 0
                        } else {
                            true
                        };

                        if show_cursor {
                            // Measure text width up to cursor column
                            let text_before: String = line.chars().take(col).collect();
                            let width_before = painter.layout_no_wrap(
                                text_before,
                                font_id.clone(),
                                Color32::TRANSPARENT,
                            ).size().x;

                            let cursor_x = rect.min.x + width_before;
                            let cursor_y = rect.min.y + row as f32 * self.cell_height;

                            match self.cursor_style {
                                CursorStyle::Block => {
                                    // Measure one char width for block cursor
                                    let char_w = if col < line.len() {
                                        let c: String = line.chars().nth(col).unwrap_or(' ').to_string();
                                        painter.layout_no_wrap(c, font_id.clone(), Color32::TRANSPARENT).size().x
                                    } else {
                                        self.cell_height * 0.6
                                    };
                                    let cursor_rect = Rect::from_min_size(
                                        egui::pos2(cursor_x, cursor_y),
                                        Vec2::new(char_w, self.cell_height),
                                    );
                                    painter.rect_filled(cursor_rect, 0.0, self.cursor_color);
                                }
                                CursorStyle::Beam => {
                                    let cursor_rect = Rect::from_min_size(
                                        egui::pos2(cursor_x, cursor_y),
                                        Vec2::new(2.0, self.cell_height),
                                    );
                                    painter.rect_filled(cursor_rect, 0.0, self.cursor_color);
                                }
                                CursorStyle::Underline => {
                                    let char_w = if col < line.len() {
                                        let c: String = line.chars().nth(col).unwrap_or(' ').to_string();
                                        painter.layout_no_wrap(c, font_id.clone(), Color32::TRANSPARENT).size().x
                                    } else {
                                        self.cell_height * 0.6
                                    };
                                    let cursor_rect = Rect::from_min_size(
                                        egui::pos2(cursor_x, cursor_y + self.cell_height - 2.0),
                                        Vec2::new(char_w, 2.0),
                                    );
                                    painter.rect_filled(cursor_rect, 0.0, self.cursor_color);
                                }
                            }
                        }

                        if self.cursor_blink_ms > 0 {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(self.cursor_blink_ms));
                        }
                    }
                }
            }
        }

        response
    }
}
