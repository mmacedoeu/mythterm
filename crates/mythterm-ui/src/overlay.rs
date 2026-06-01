//! Overlay system (search, command palette, etc.).
//!
//! Provides overlay UI elements that appear on top of the terminal content.

use egui::{Color32, Ui, Vec2};

/// Search overlay for finding text in the terminal.
pub struct SearchOverlay {
    /// The search query.
    pub query: String,
    /// Whether to use regex.
    pub use_regex: bool,
    /// Number of matches found.
    pub match_count: usize,
    /// Current match index.
    pub current_match: usize,
}

impl SearchOverlay {
    /// Create a new search overlay.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            use_regex: false,
            match_count: 0,
            current_match: 0,
        }
    }

    /// Show the search overlay.
    pub fn show(&mut self, ctx: &egui::Context) -> SearchAction {
        let mut action = SearchAction::None;

        egui::Window::new("Search")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let response = ui.text_edit_singleline(&mut self.query);
                    if response.changed() {
                        action = SearchAction::QueryChanged;
                    }

                    ui.checkbox(&mut self.use_regex, "Regex");

                    if ui.button("↑").clicked() {
                        action = SearchAction::Previous;
                    }

                    if ui.button("↓").clicked() {
                        action = SearchAction::Next;
                    }

                    ui.label(format!("{}/{}", self.current_match, self.match_count));

                    if ui.button("✕").clicked() {
                        action = SearchAction::Close;
                    }
                });
            });

        action
    }
}

/// Actions from the search overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchAction {
    None,
    QueryChanged,
    Next,
    Previous,
    Close,
}

/// Command palette overlay.
pub struct CommandPalette {
    /// The search query.
    pub query: String,
    /// Available commands.
    pub commands: Vec<Command>,
    /// Selected command index.
    pub selected: usize,
}

/// A command in the command palette.
#[derive(Debug, Clone)]
pub struct Command {
    /// Command name.
    pub name: String,
    /// Command description.
    pub description: String,
    /// Keyboard shortcut.
    pub shortcut: Option<String>,
}

impl CommandPalette {
    /// Create a new command palette.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            commands: vec![
                Command {
                    name: "New Tab".to_string(),
                    description: "Open a new tab".to_string(),
                    shortcut: Some("Ctrl+Shift+T".to_string()),
                },
                Command {
                    name: "Close Tab".to_string(),
                    description: "Close the current tab".to_string(),
                    shortcut: Some("Ctrl+Shift+W".to_string()),
                },
                Command {
                    name: "Split Horizontal".to_string(),
                    description: "Split the current pane horizontally".to_string(),
                    shortcut: Some("Ctrl+Shift+D".to_string()),
                },
                Command {
                    name: "Split Vertical".to_string(),
                    description: "Split the current pane vertically".to_string(),
                    shortcut: Some("Ctrl+Shift+E".to_string()),
                },
                Command {
                    name: "Search".to_string(),
                    description: "Search in terminal output".to_string(),
                    shortcut: Some("Ctrl+Shift+F".to_string()),
                },
            ],
            selected: 0,
        }
    }

    /// Show the command palette and return the selected command, if any.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<String> {
        let mut selected = None;

        egui::Window::new("Command Palette")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(">");
                    ui.text_edit_singleline(&mut self.query);
                });

                ui.separator();

                let filtered: Vec<_> = self
                    .commands
                    .iter()
                    .filter(|cmd| {
                        self.query.is_empty()
                            || cmd
                                .name
                                .to_lowercase()
                                .contains(&self.query.to_lowercase())
                    })
                    .collect();

                for (i, cmd) in filtered.iter().enumerate() {
                    let is_selected = i == self.selected;

                    let bg_color = if is_selected {
                        Color32::from_rgb(60, 60, 60)
                    } else {
                        Color32::TRANSPARENT
                    };

                    let response = ui.allocate_response(
                        Vec2::new(ui.available_width(), 24.0),
                        egui::Sense::click(),
                    );

                    if ui.is_rect_visible(response.rect) {
                        ui.painter().rect_filled(response.rect, 0.0, bg_color);
                        ui.painter().text(
                            response.rect.left_center() + Vec2::new(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &cmd.name,
                            egui::FontId::proportional(13.0),
                            Color32::from_rgb(200, 200, 200),
                        );

                        if let Some(shortcut) = &cmd.shortcut {
                            ui.painter().text(
                                response.rect.right_center() - Vec2::new(8.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                shortcut,
                                egui::FontId::proportional(11.0),
                                Color32::from_rgb(120, 120, 120),
                            );
                        }
                    }

                    if response.clicked() {
                        selected = Some(cmd.name.clone());
                    }
                }
            });

        selected
    }
}
