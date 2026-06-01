//! Tab bar widget.
//!
//! Displays a horizontal strip of tabs at the top of the window.

use egui::{Color32, Response, Sense, Ui, Vec2, Widget};

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
            height: 32.0,
        }
    }

    /// Set the tab bar height.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Show the tab bar and return the clicked tab index, if any.
    pub fn show(&self, ui: &mut Ui) -> Option<usize> {
        let mut clicked = None;

        ui.horizontal(|ui| {
            for (i, title) in self.titles.iter().enumerate() {
                let is_active = i == self.active;

                let bg_color = if is_active {
                    Color32::from_rgb(50, 50, 50)
                } else {
                    Color32::from_rgb(35, 35, 35)
                };

                let text_color = if is_active {
                    Color32::from_rgb(220, 220, 220)
                } else {
                    Color32::from_rgb(140, 140, 140)
                };

                let response = ui.allocate_ui_with_layout(
                    Vec2::new(120.0, self.height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(120.0, self.height), Sense::click());

                        if ui.is_rect_visible(rect) {
                            ui.painter().rect_filled(rect, 0.0, bg_color);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                title,
                                egui::FontId::proportional(13.0),
                                text_color,
                            );
                        }

                        response
                    },
                );

                if response.inner.clicked() {
                    clicked = Some(i);
                }
            }

            // "+" button for new tab
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(self.height, self.height), Sense::click());

            if ui.is_rect_visible(rect) {
                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(35, 35, 35));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(16.0),
                    Color32::from_rgb(140, 140, 140),
                );
            }

            if response.clicked() {
                clicked = Some(self.titles.len()); // New tab index
            }
        });

        clicked
    }
}

impl Widget for TabBar {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut clicked = None;

        ui.horizontal(|ui| {
            for (i, title) in self.titles.iter().enumerate() {
                let is_active = i == self.active;

                let bg_color = if is_active {
                    Color32::from_rgb(50, 50, 50)
                } else {
                    Color32::from_rgb(35, 35, 35)
                };

                let text_color = if is_active {
                    Color32::from_rgb(220, 220, 220)
                } else {
                    Color32::from_rgb(140, 140, 140)
                };

                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(120.0, self.height), Sense::click());

                if ui.is_rect_visible(rect) {
                    ui.painter().rect_filled(rect, 0.0, bg_color);
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
        });

        // Return a dummy response since we handle clicks internally
        ui.allocate_response(Vec2::ZERO, Sense::hover())
    }
}
