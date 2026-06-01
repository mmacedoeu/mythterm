//! Input handling: maps egui/winit events to terminal VT sequences.
//!
//! Captures keyboard and mouse events from the egui event loop and
//! encodes them as VT sequences to send to the PTY.

use egui::Key;

/// Maps egui key events to VT escape sequences.
pub struct InputMapper {
    /// Whether the terminal is in application cursor mode.
    app_cursor_mode: bool,
    /// Whether bracketed paste mode is enabled.
    bracketed_paste: bool,
}

impl InputMapper {
    /// Create a new input mapper.
    pub fn new() -> Self {
        Self {
            app_cursor_mode: false,
            bracketed_paste: false,
        }
    }

    /// Set application cursor mode.
    pub fn set_app_cursor_mode(&mut self, enabled: bool) {
        self.app_cursor_mode = enabled;
    }

    /// Set bracketed paste mode.
    pub fn set_bracketed_paste(&mut self, enabled: bool) {
        self.bracketed_paste = enabled;
    }

    /// Convert an egui key event to a VT sequence.
    ///
    /// Returns the bytes to send to the PTY, or None if the key
    /// should be handled by the UI instead.
    pub fn map_key(
        &self,
        key: Key,
        modifiers: &egui::Modifiers,
        text: Option<&str>,
    ) -> Option<Vec<u8>> {
        // Handle Ctrl+key combinations
        if modifiers.ctrl {
            return self.map_ctrl_key(key);
        }

        // Handle special keys
        match key {
            Key::Enter => Some(b"\r".to_vec()),
            Key::Tab => Some(b"\t".to_vec()),
            Key::Backspace => Some(b"\x7f".to_vec()),
            Key::Escape => Some(b"\x1b".to_vec()),
            Key::ArrowUp => {
                if self.app_cursor_mode {
                    Some(b"\x1bOA".to_vec())
                } else {
                    Some(b"\x1b[A".to_vec())
                }
            }
            Key::ArrowDown => {
                if self.app_cursor_mode {
                    Some(b"\x1bOB".to_vec())
                } else {
                    Some(b"\x1b[B".to_vec())
                }
            }
            Key::ArrowRight => {
                if self.app_cursor_mode {
                    Some(b"\x1bOC".to_vec())
                } else {
                    Some(b"\x1b[C".to_vec())
                }
            }
            Key::ArrowLeft => {
                if self.app_cursor_mode {
                    Some(b"\x1bOD".to_vec())
                } else {
                    Some(b"\x1b[D".to_vec())
                }
            }
            Key::Home => Some(b"\x1b[H".to_vec()),
            Key::End => Some(b"\x1b[F".to_vec()),
            Key::PageUp => Some(b"\x1b[5~".to_vec()),
            Key::PageDown => Some(b"\x1b[6~".to_vec()),
            Key::Insert => Some(b"\x1b[2~".to_vec()),
            Key::Delete => Some(b"\x1b[3~".to_vec()),
            Key::F1 => Some(b"\x1bOP".to_vec()),
            Key::F2 => Some(b"\x1bOQ".to_vec()),
            Key::F3 => Some(b"\x1bOR".to_vec()),
            Key::F4 => Some(b"\x1bOS".to_vec()),
            Key::F5 => Some(b"\x1b[15~".to_vec()),
            Key::F6 => Some(b"\x1b[17~".to_vec()),
            Key::F7 => Some(b"\x1b[18~".to_vec()),
            Key::F8 => Some(b"\x1b[19~".to_vec()),
            Key::F9 => Some(b"\x1b[20~".to_vec()),
            Key::F10 => Some(b"\x1b[21~".to_vec()),
            Key::F11 => Some(b"\x1b[23~".to_vec()),
            Key::F12 => Some(b"\x1b[24~".to_vec()),
            _ => {
                // For printable characters, use the text input
                if let Some(text) = text {
                    Some(text.as_bytes().to_vec())
                } else {
                    None
                }
            }
        }
    }

    /// Map Ctrl+key combinations to VT sequences.
    fn map_ctrl_key(&self, key: Key) -> Option<Vec<u8>> {
        match key {
            Key::A => Some(b"\x01".to_vec()),
            Key::B => Some(b"\x02".to_vec()),
            Key::C => Some(b"\x03".to_vec()),
            Key::D => Some(b"\x04".to_vec()),
            Key::E => Some(b"\x05".to_vec()),
            Key::F => Some(b"\x06".to_vec()),
            Key::G => Some(b"\x07".to_vec()),
            Key::H => Some(b"\x08".to_vec()),
            Key::I => Some(b"\x09".to_vec()),
            Key::J => Some(b"\x0a".to_vec()),
            Key::K => Some(b"\x0b".to_vec()),
            Key::L => Some(b"\x0c".to_vec()),
            Key::M => Some(b"\x0d".to_vec()),
            Key::N => Some(b"\x0e".to_vec()),
            Key::O => Some(b"\x0f".to_vec()),
            Key::P => Some(b"\x10".to_vec()),
            Key::Q => Some(b"\x11".to_vec()),
            Key::R => Some(b"\x12".to_vec()),
            Key::S => Some(b"\x13".to_vec()),
            Key::T => Some(b"\x14".to_vec()),
            Key::U => Some(b"\x15".to_vec()),
            Key::V => Some(b"\x16".to_vec()),
            Key::W => Some(b"\x17".to_vec()),
            Key::X => Some(b"\x18".to_vec()),
            Key::Y => Some(b"\x19".to_vec()),
            Key::Z => Some(b"\x1a".to_vec()),
            _ => None,
        }
    }

    /// Encode mouse event as VT sequence.
    ///
    /// Returns the bytes to send to the PTY.
    pub fn encode_mouse_event(
        &self,
        button: MouseButton,
        action: MouseAction,
        x: u16,
        y: u16,
        modifiers: &egui::Modifiers,
    ) -> Vec<u8> {
        let cb = match (button, action) {
            (MouseButton::Left, MouseAction::Press) => 0,
            (MouseButton::Middle, MouseAction::Press) => 1,
            (MouseButton::Right, MouseAction::Press) => 2,
            (_, MouseAction::Release) => 3, // Any button release
            (MouseButton::ScrollUp, _) => 64,
            (MouseButton::ScrollDown, _) => 65,
        };

        let cm = if modifiers.shift { 4 } else { 0 }
            | if modifiers.alt { 8 } else { 0 }
            | if modifiers.ctrl { 16 } else { 0 };

        // SGR extended coordinates format
        format!("\x1b[<{};{};{}{}", cb + cm, x + 1, y + 1, match action {
            MouseAction::Press => 'M',
            MouseAction::Release => 'm',
        })
        .into_bytes()
    }

    /// Encode paste text with bracketing if enabled.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let mut result = Vec::new();

        if self.bracketed_paste {
            result.extend_from_slice(b"\x1b[200~");
        }

        result.extend_from_slice(text.as_bytes());

        if self.bracketed_paste {
            result.extend_from_slice(b"\x1b[201~");
        }

        result
    }
}

impl Default for InputMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    ScrollUp,
    ScrollDown,
}

/// Mouse action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
}
