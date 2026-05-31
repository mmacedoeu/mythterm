//! mythterm-core: Terminal emulator core
//!
//! Provides the VT escape sequence parser, screen model, cell grid,
//! scrollback buffer, and terminal state machine.

pub mod cell;
pub mod screen;
pub mod terminal;
pub mod terminalstate;

pub use cell::{Cell, CellAttributes, ColorAttribute};
pub use screen::Screen;
pub use terminal::Terminal;
