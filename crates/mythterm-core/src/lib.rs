//! mythterm-core: Thin re-export of WezTerm's terminal emulator core.
//!
//! This crate re-exports the following WezTerm crates as-is via git dependency:
//! - `wezterm-term` — VT parser, screen model, terminal state machine
//! - `wezterm-cell` — Cell/CellAttributes data structures
//! - `wezterm-surface` — Line/Surface types, scrollback
//! - `termwiz` — Escape parser, input encoding, surface model
//! - `portable-pty` — Cross-platform PTY management
//! - `wezterm-bidi` — Bidirectional text support
//! - `wezterm-color-types` — Color types
//! - `wezterm-input-types` — Input type definitions
//! - `wezterm-dynamic` — Dynamic JSON-like type system
//! - `wezterm-escape-parser` — Escape sequence parser
//! - `wezterm-char-props` — Unicode character properties
//! - `wezterm-blob-leases` — Image blob caching
//! - `vtparse` — VT parser state machine
//! - `strip-ansi-escapes` — ANSI escape stripping

// Core terminal engine
pub use wezterm_term::*;

// Cell data structures
pub use wezterm_cell;

// Line/Surface types
pub use wezterm_surface;

// Terminal wizardry (escape parser, input encoding)
pub use termwiz;

// PTY management
pub use portable_pty;

// Supporting crates
pub use wezterm_bidi;
pub use wezterm_blob_leases;
pub use wezterm_char_props;
pub use wezterm_color_types;
pub use wezterm_dynamic;
pub use wezterm_escape_parser;
pub use wezterm_input_types;
pub use vtparse;
