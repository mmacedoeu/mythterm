# mythterm — Comprehensive Porting Plan

## Table of Contents

1. [Overview](#1-overview)
2. [Source Analysis](#2-source-analysis)
3. [Architecture](#3-architecture)
4. [Crate Map](#4-crate-map)
5. [Phase 0 — Scaffolding & Build](#5-phase-0--scaffolding--build)
6. [Phase 1 — Terminal Core (Reuse)](#6-phase-1--terminal-core-reuse-as-is)
7. [Phase 2 — Font Pipeline](#7-phase-2--font-pipeline)
8. [Phase 3 — Multiplexer (Fork)](#8-phase-3--multiplexer-fork-from-wezterm)
9. [Phase 4 — Rendering Engine Integration](#9-phase-4--rendering-engine-integration)
10. [Phase 5 — UI Layer (egui)](#10-phase-5--ui-layer-egui)
11. [Phase 6 — Input Handling](#11-phase-6--input-handling)
12. [Phase 7 — Configuration](#12-phase-7--configuration)
13. [Phase 8 — Advanced Features](#13-phase-8--advanced-features)
14. [Phase 9 — Platform & Packaging](#14-phase-9--platform--packaging)
15. [Phase 10 — Performance & Polish](#15-phase-10--performance--polish)
16. [Dependency Graph](#16-dependency-graph)
17. [Testing Strategy](#17-testing-strategy)
18. [Risk Register](#18-risk-register)

---

## 1. Overview

**mythterm** is a GPU-accelerated terminal emulator that replaces WezTerm's
custom OpenGL/wgpu rendering pipeline with the
[Myth engine](https://github.com/panxinmiao/myth) for GPU rendering and
[egui](https://github.com/emilk/egui) for the UI chrome layer.

### Goals

- Reuse WezTerm's core terminal features (VT parsing, scrollback,
  images, hyperlinks, sixel, mouse tracking, selection) via git dependency.
- Use Myth's SSA-based render graph for GPU rendering instead of
  WezTerm's hand-rolled wgpu/OpenGL quad pipeline.
- Use egui for all UI chrome: tab bar, split borders, settings, overlays,
  command palette, right-click menus.
- Maintain WezTerm's performance characteristics (60fps scrolling,
  sub-frame latency).
- Cross-platform: Linux (primary), macOS, Windows. WASM is a stretch goal that would require wasm-bindgen for Myth/egui and is out of scope for v0.1.

### Non-Goals (v0.1)

- Lua scripting engine (defer beyond v0.1).
- SSH multiplexing / remote domains (defer beyond v0.1).
- tmux integration (defer beyond v0.1).
- Plugin system (defer beyond v0.1).

---

## 2. Source Analysis

### 2.1 WezTerm Crate Reuse Analysis

After auditing the WezTerm source at `../tools/wezterm`, most non-GUI crates
can be used as-is via git dependencies. Only rendering, windowing, config,
and font crates need replacement.

#### Reuse as-is (git dependency from wezterm repo)

These crates are pure Rust with no platform/GUI coupling:

| Crate | Purpose | Lines | Notes |
|-------|---------|-------|-------|
| `wezterm-term` | VT parser, screen model, terminal state machine | ~10,700 | **Core terminal engine** |
| `wezterm-cell` | Cell/CellAttributes data structures | ~800 | Feature-gated, optional serde |
| `wezterm-surface` | Line/Surface types, scrollback | ~1,200 | Core abstraction |
| `termwiz` | Escape parser, input encoding, surface model | ~6,000 | Full terminal wizardry |
| `wezterm-escape-parser` | Escape sequence parser | ~2,500 | Pure Rust, feature-gated |
| `wezterm-dynamic` | Dynamic JSON-like type system | ~1,000 | Used by config/term |
| `wezterm-dynamic-derive` | Derive macros for above | ~200 | Proc macro |
| `wezterm-color-types` | Color types (Srgba, Hsla, etc.) | ~400 | Pure Rust |
| `wezterm-char-props` | Unicode character properties | ~500 | Width tables, categories |
| `wezterm-input-types` | Input type definitions (KeyCode, Modifiers) | ~300 | Pure Rust |
| `wezterm-bidi` | Bidirectional text support | ~400 | Unicode bidi algorithm |
| `wezterm-blob-leases` | Image blob caching/leasing | ~300 | Pure Rust |
| `vtparse` | VT parser state machine | ~800 | Low-level parser |
| `portable-pty` | Cross-platform PTY management | ~1,200 | Platform-specific but self-contained |
| `wezterm-toast-notification` | OS toast notifications | ~300 | Platform-specific but self-contained |
| `wezterm-open-url` | Open URL in browser | ~30 | Platform-specific |
| `strip-ansi-escapes` | ANSI escape stripping | ~100 | Tiny utility |
| `bintree` | Binary tree data structure | ~100 | Transitive dep |
| `rangeset` | Range set operations | ~200 | Transitive dep |
| `ratelim` | Rate limiter | ~100 | Transitive dep |
| `frecency` | Frecency scoring | ~100 | Transitive dep |
| `lfucache` | LFU cache | ~100 | Transitive dep |

#### Fork and modify

| Crate | Purpose | Why fork | Strategy |
|-------|---------|----------|----------|
| `mux` | Multiplexer (tabs, panes, domains) | Depends on `config` (Lua/mlua), `wezterm-ssh` | Fork into `mythterm-mux`, strip Lua/SSH deps, replace `config::` calls with trait-based interface |
| `config` | Lua/TOML config, color schemes | Deeply coupled to Lua runtime (mlua) | Build new `mythterm-config` (TOML-only, no Lua) |

#### Replace entirely

| Crate | Purpose | Why replace | Target |
|-------|---------|------------|--------|
| `wezterm-gui` | GPU rendering, glyph cache, quad pipeline | Entire GUI layer | `mythterm-render` (Myth engine) |
| `window` | Cross-platform windowing, OpenGL context | Platform windowing | `myth-app` (Myth engine) |
| `wezterm-font` | Font discovery, shaping (harfbuzz), rasterization (freetype) | C library deps (freetype, harfbuzz, cairo) | `mythterm-font` (ab_glyph/rustybuzz) |
| `codec` | Mux server protocol | Depends on mux+config | Deferred |
| `wezterm-client` | Remote mux client | Depends on codec | Deferred |
| `wezterm-ssh` | SSH client | Heavy deps | Deferred |

### 2.2 WezTerm Rendering Pipeline (what we replace)

WezTerm's `wezterm-gui/src/termwindow/render/` does:

1. **Glyph Cache** (`glyphcache.rs`): Rasterizes glyphs via freetype/coretext,
   packs them into a texture atlas (guillotiere bin-packing).
2. **Quad Rendering** (`quad.rs`): Each cell is a textured quad. Background
   colors are separate quads. Uses instanced rendering.
3. **Shape Cache** (`shapecache.rs`): Caches shaped text runs (harfbuzz).
4. **Draw Pipeline** (`draw.rs`, `paint.rs`): Iterates visible lines, clusters
   cells by attributes, emits quads.
5. **Custom Shaders** (`shader.wgsl`, `glyph-*.glsl`): GPU shaders for text
   rendering with subpixel positioning.
6. **Tab Bar** (`tab_bar.rs`, `fancy_tab_bar.rs`): Rendered as quads.
7. **Selection** (`selection.rs`): Highlight overlay quads.
8. **Scrollbar** (`scrollbar.rs`): Overlay quad strip.

### 2.3 Myth Engine Capabilities

Myth provides:

- **wgpu device management** via `myth_render` (GPU context, pipelines, bind groups)
- **SSA-based render graph** (`myth_render::graph`): Declarative pass DAG with
  automatic synchronization, memory aliasing, dead-pass elimination.
- **Scene graph** (`myth_scene`): Meshes, materials, lights, cameras, transforms.
- **App framework** (`myth_app`): winit-based event loop, window management.
- **Asset pipeline** (`myth_assets`): Async asset loading, handle-based management.
- **Shader generation** (`myth_render::pipeline::shader_gen`): Template-based
  WGSL shader generation.
- **egui integration**: Already a dev-dependency in myth's workspace.

### 2.4 Key Architectural Difference

```
WezTerm:                          mythterm:
┌─────────────┐                   ┌─────────────┐
│  wezterm-gui │                   │  mythterm-ui │  egui chrome
│  (quad GL)   │                   │  (egui)      │  (tabs, splits, settings)
├─────────────┤                   ├─────────────┤
│  window      │                   │  myth-app    │  winit + event loop
│  (raw GL)    │                   │  (myth)      │
├─────────────┤                   ├─────────────┤
│  term        │                   │  mythterm-   │  Custom render pipeline
│  (VT core)   │                   │  render      │  on myth's render graph
├─────────────┤                   ├─────────────┤
│  mux         │                   │  mythterm-   │  Forked, stripped
│  (PTY/tabs)  │                   │  mux         │  Lua/SSH deps
├─────────────┤                   ├─────────────┤
│  wezterm-font│                   │  mythterm-   │  rustybuzz + ab_glyph
│  (harfbuzz)  │                   │  font        │
├─────────────┤                   ├─────────────┤
│  config      │                   │  mythterm-   │  TOML + live reload
│  (Lua/TOML)  │                   │  config      │
└─────────────┘                   └─────────────┘
```

---

## 3. Architecture

### 3.1 Rendering Strategy

**Two-layer rendering approach:**

Layer 1 — **Myth Render Graph** (terminal content):
- Register a custom `TerminalPass` into Myth's render graph.
- This pass renders the terminal grid (backgrounds, text glyphs, cursor,
  selection, images) as textured quads using a custom WGSL shader.
- The pass outputs to a texture that Myth's compositor can blend.

Layer 2 — **egui** (UI chrome):
- egui renders on top of the terminal content.
- Tab bar, split borders, status bar, overlays, command palette.
- Uses `egui-wgpu` for GPU rendering, composited by Myth.

**Composition:**
```
Myth Render Graph
├── TerminalPass (custom) → terminal_content_texture
├── ScenePass (optional, for 3D effects)
└── CompositePass
    ├── terminal_content_texture (bottom layer)
    └── egui_pass (top layer, UI chrome)
```

### 3.2 Text Rendering Pipeline

```
Font Discovery (fontconfig/OS)
        │
        ▼
Font Loading (ab_glyph)
        │
        ▼
Text Shaping (rustybuzz)
  ┌─────┴─────┐
  │  Run of    │  cluster of cells with same attributes
  │  shaped    │  → glyph IDs + positions + advances
  │  glyphs    │
  └─────┬─────┘
        │
        ▼
Glyph Rasterization (ab_glyph)
  ┌─────┴─────┐
  │  SDF or    │  rasterize to alpha bitmaps
  │  bitmap    │  pack into glyph atlas texture
  └─────┬─────┘
        │
        ▼
Glyph Atlas (GPU texture)
  ┌─────┴─────┐
  │  Packed    │  managed via guillotiere or similar
  │  glyphs    │  evict on overflow (LRU)
  └─────┬─────┘
        │
        ▼
Quad Generation
  ┌─────┴─────┐
  │  Per-cell  │  position + UV coords + color
  │  quads     │  instance buffer for GPU
  └─────┬─────┘
        │
        ▼
TerminalPass (WGSL shader)
  ┌─────┴─────┐
  │  Vertex    │  background color pass
  │  + frag    │  glyph texture pass (alpha blending)
  │  shaders   │  cursor/selection overlay pass
  └───────────┘
```

### 3.3 Data Flow

```
PTY stdout → mythterm-core (VT parser → cell grid)
                                │
                                ▼
                    mythterm-render (dirty diff → quad generation → GPU)
                                │
                                ▼
                    mythterm-ui (egui frame → UI chrome)
                                │
                                ▼
                    myth-app (composite → window surface)

PTY stdin  ← mythterm-core (keyboard input → VT sequences)
                ↑
                │
            mythterm-ui (key events from egui/winit)
```

---

## 4. Crate Map

```
mythterm/
├── Cargo.toml                    # Workspace root
├── PLAN.md                       # This document
├── README.md
├── mythterm-bin/
│   └── src/main.rs               # Binary entry point
│                                 #   - arg parsing (clap)
│                                 #   - myth engine init
│                                 #   - event loop wiring
│                                 #   - app lifecycle
└── crates/
    ├── mythterm-core/            # Thin re-export of WezTerm terminal core
    │   └── src/
    │       └── lib.rs             # Re-exports: wezterm-term, wezterm-cell,
    │                              #   wezterm-surface, termwiz, portable-pty
    │
    ├── mythterm-config/          # Configuration
    │   └── src/
    │       ├── lib.rs
    │       ├── settings.rs       # Settings struct (TOML)
    │       ├── scheme.rs         # ColorScheme definitions
    │       ├── keybinding.rs     # Key binding config
    │       └── reload.rs         # File watcher + live reload
    │
    ├── mythterm-font/            # Font system
    │   └── src/
    │       ├── lib.rs
    │       ├── discovery.rs      # Font discovery (fontconfig, OS dirs)
    │       ├── loader.rs         # Font file loading (ab_glyph)
    │       ├── shape.rs          # Text shaping (rustybuzz)
    │       ├── rasterize.rs      # Glyph rasterization
    │       └── metrics.rs        # Font metrics, cell size calculation
    │
    ├── mythterm-mux/             # Forked from WezTerm mux (stripped Lua/SSH)
    │   └── src/
    │       ├── lib.rs            # Mux/Session (top-level container)
    │       ├── activity.rs       # Activity tracking
    │       ├── domain.rs         # Domain abstraction (LocalDomain)
    │       ├── localpane.rs      # PTY-backed pane
    │       ├── pane.rs           # Pane trait
    │       ├── renderable.rs     # Renderable pane interface
    │       ├── tab.rs            # Tab (pane container)
    │       └── window.rs         # Window (tab container)
    │
    ├── mythterm-render/          # Myth engine integration
    │   └── src/
    │       ├── lib.rs
    │       ├── renderer.rs       # TerminalRenderer (orchestrator)
    │       ├── atlas/
    │       │   ├── mod.rs
    │       │   ├── glyph_atlas.rs  # Glyph texture atlas
    │       │   ├── image_atlas.rs  # Sixel/image texture atlas
    │       │   └── packer.rs       # Guillotiere bin-packing
    │       ├── pipeline/
    │       │   ├── mod.rs
    │       │   ├── terminal_pass.rs  # Custom terminal render pass
    │       │   ├── text_shader.rs    # WGSL text shader
    │       │   ├── bg_shader.rs      # WGSL background shader
    │       │   └── compositor.rs     # Layer compositing
    │       ├── quad.rs           # Quad vertex types + instance buffer
    │       └── dirty.rs          # Dirty tracking, incremental updates
    │
    └── mythterm-ui/              # egui UI layer
        └── src/
            ├── lib.rs
            ├── terminal_widget.rs  # egui widget: renders terminal texture
            ├── tabbar.rs           # Tab bar widget
            ├── splits.rs           # Split pane layout manager
            ├── scrollbar.rs        # Scrollbar overlay
            ├── selection.rs        # Selection rendering (egui painter)
            ├── overlay.rs          # Overlay system (search, goto)
            ├── command_palette.rs  # Command palette (fuzzy search)
            ├── context_menu.rs     # Right-click context menu
            ├── settings_ui.rs      # Settings panel
            └── toast.rs            # Toast notification system
```

---

## 5. Phase 0 — Scaffolding & Build

**Goal:** Get the workspace compiling with all crates as stubs.

### Tasks

- [x] Create GitHub repo `mmacedoeu/mythterm`
- [x] Initialize Cargo workspace with 7 crates
- [x] Create stub `lib.rs` / `main.rs` for each crate
- [x] Set myth as path dependency (`../frontend/myth`)
- [ ] Verify `cargo check` passes for the full workspace
- [ ] Add CI workflow (`.github/workflows/ci.yml`):
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
  - `cargo test`
  - `cargo build --release`
- [ ] Add `rust-toolchain.toml` (pin Rust 1.92+, edition 2024)
- [ ] Add `deny.toml` for dependency auditing

### Verification

```bash
cargo check 2>&1 | grep "error" | wc -l  # should be 0
```

---

## 6. Phase 1 — Terminal Core (Reuse as-is)

**Goal:** Use WezTerm's terminal core as a direct git dependency.

The entire `term` crate (~10,700 lines) and its dependency chain
(`wezterm-cell`, `wezterm-surface`, `termwiz`, `wezterm-escape-parser`,
`wezterm-dynamic`, `wezterm-color-types`, `wezterm-char-props`,
`wezterm-input-types`, `wezterm-bidi`, `wezterm-blob-leases`, `vtparse`)
are pure Rust with no GUI/platform coupling. They can be used as-is.

### Tasks

- [ ] Add git dependencies to workspace Cargo.toml (pinned to commit `577474d`):
  ```toml
  # WezTerm terminal core (pinned to 577474d89ee61aef4a48145cdec82a638d874751)
  wezterm-term    = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["use_serde"] }
  wezterm-cell    = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["std", "use_serde", "use_image"] }
  wezterm-surface = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["std", "appdata", "use_serde", "use_image"] }
  termwiz         = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["use_serde", "use_image"] }
  wezterm-escape-parser = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["std"] }
  wezterm-dynamic = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["std"] }
  wezterm-dynamic-derive = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  wezterm-color-types = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["std", "use_serde"] }
  wezterm-char-props = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  wezterm-input-types = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  wezterm-bidi    = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  wezterm-blob-leases = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  vtparse         = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  portable-pty    = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d", features = ["serde_support"] }
  strip-ansi-escapes = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  wezterm-toast-notification = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  wezterm-open-url = { git = "https://github.com/wezterm/wezterm.git", rev = "577474d" }
  ```
- [ ] Verify `cargo check -p mythterm-core` passes (mythterm-core just re-exports)
- [ ] Create `mythterm-core` as a thin re-export crate:
  ```rust
  // mythterm-core/src/lib.rs
  pub use wezterm_term::*;
  pub use wezterm_cell;
  pub use wezterm_surface;
  pub use termwiz;
  pub use portable_pty;
  ```
- [ ] Implement `TerminalConfiguration` trait for mythterm-config integration

### What we get for free

- **VT parser** with full CSI/OSC/DCS support (~2,766 lines in `terminalstate/mod.rs`)
- **CSI performer** (~1,109 lines in `terminalstate/performer.rs`)
- **Kitty image protocol** (~979 lines)
- **Screen buffer** with scrollback ring (~1,155 lines)
- **Mouse tracking** (X10, Normal, SGR, URXVT) (~364 lines)
- **Sixel graphics** (~156 lines)
- **iTerm2 inline images** (~153 lines)
- **Cell/Line/Surface** data structures with compact representation
- **Input encoding** (keyboard modes, modifyOtherKeys, etc.)
- **Bidirectional text** support
- **Image blob management** (sixel, kitty, iterm2)
- **PTY management** via `portable-pty`
- **Escape sequence parsing** via `termwiz` + `wezterm-escape-parser`

### Verification

```bash
cargo check -p mythterm-core

# Run WezTerm's existing test suite
cargo test -p wezterm-term
```

---

## 7. Phase 2 — Font Pipeline

**Goal:** Replace WezTerm's freetype/harfbuzz pipeline with rustybuzz + ab_glyph.

### Source Files to Reference

From `wezterm/wezterm-font/src/`:

| File | Action |
|------|--------|
| `lib.rs` (FontConfiguration) | Port the config interface |
| `shaper/*.rs` | Replace with rustybuzz |
| `rasterize/*.rs` | Replace with ab_glyph |
| `locator/*.rs` | Port font discovery logic |
| `units.rs` | Port unit types |

### Sub-Tasks

#### 2a. Font Discovery

- [ ] Implement fontconfig-based discovery (Linux)
- [ ] Implement CoreText-based discovery (macOS) — stub for now
- [ ] Implement DirectWrite-based discovery (Windows) — stub for now
- [ ] Implement fallback font chain (primary → emoji → CJK → symbol)
- [ ] Parse font family names, weight, style from config
- [ ] Load font files into memory

#### 2b. Text Shaping

- [ ] Initialize rustybuzz `Face` from font data
- [ ] Implement `shape_text(text, font, features) → Vec<ShapedGlyph>`
- [ ] Handle ligatures (calt, liga OpenType features)
- [ ] Handle emoji presentation (text vs emoji selector)
- [ ] Handle CJK width (full-width vs half-width)
- [ ] Handle bidi reordering (pass bidi levels from core)
- [ ] Implement shaping cache (text → shaped glyphs, LRU)
- [ ] Handle font fallback within a shaping run

#### 2c. Glyph Rasterization

- [ ] Implement rasterization with ab_glyph
- [ ] Support both bitmap (SDF optional) and outline rasterization
- [ ] Handle subpixel positioning (multiple rasterizations per glyph)
- [ ] Handle LCD subpixel rendering (RGB/BGR ordering)
- [ ] Compute font metrics (ascent, descent, line gap, cell width/height)
- [ ] Implement `RenderMetrics` equivalent (cell size, underline position)

#### 2d. Font Configuration

- [ ] Implement `FontConfiguration` (resolves font names → loaded fonts)
- [ ] Handle font size changes
- [ ] Handle font weight/style variations (bold, italic)
- [ ] Implement font stretch/squeeze for box-drawing alignment
- [ ] Cache loaded fonts by key

### Key Differences from WezTerm

| Aspect | WezTerm | mythterm |
|--------|---------|----------|
| Shaping engine | HarfBuzz (C) | rustybuzz (pure Rust) |
| Rasterizer | FreeType / CoreText | ab_glyph (pure Rust) |
| Metrics | `wezterm-font::units` | ab_glyph `PxScale` |
| Fallback | Custom + fontconfig | Custom + fontconfig |

### Verification

```bash
cargo test -p mythterm-font

# Visual: render "Hello 世界 🌍" and verify correct shaping
# Visual: render box-drawing characters (─ │ ┌ ┐ └ ┘) and verify alignment
```

---

## 8. Phase 3 — Multiplexer (Fork from WezTerm)

**Goal:** Fork WezTerm's `mux` crate, strip Lua/SSH dependencies.

The `mux` crate has 42 references to `config::` and 5 to `mlua`/`luahelper`.
Strategy: fork into `mythterm-mux`, remove deferred modules, replace config
calls with a trait-based interface backed by `mythterm-config`.

### Modules to strip

| Module | Lines | Reason |
|--------|-------|--------|
| `ssh.rs` | ~400 | Deferred (heavy deps) |
| `ssh_agent.rs` | ~100 | Deferred |
| `client.rs` | ~200 | Deferred (remote mux) |
| `connui.rs` | ~100 | Deferred (connection UI) |
| `tmux.rs` | ~300 | Deferred (tmux integration) |
| `tmux_commands.rs` | ~400 | Deferred |
| `tmux_pty.rs` | ~100 | Deferred |

### Config decoupling tasks

- [ ] Copy `mux/src/` into `crates/mythterm-mux/src/`
- [ ] Remove ssh.rs, ssh_agent.rs, client.rs, connui.rs, tmux*.rs
- [ ] Define `MuxConfig` trait to replace `config::configuration()`:
  ```rust
  pub trait MuxConfig: Send + Sync {
      fn exit_behavior(&self) -> ExitBehavior;
      fn default_shell(&self) -> &str;
      fn enable_tab_bar_at_bottom(&self) -> bool;
      fn mux_enable_ssh_agent(&self) -> bool;
      // ... other config values mux needs
  }
  ```
- [ ] Replace `config::configuration()` calls with trait method calls
- [ ] Replace `config::keyassignment::*` types with local type definitions
- [ ] Remove `mlua` and `luahelper` dependencies
- [ ] Replace Lua callback hooks in `domain.rs` and `localpane.rs` with event/trait pattern
- [ ] Implement `MuxConfig` in `mythterm-config`

### Sub-Tasks

#### 3a. PTY Management

- [ ] Port `LocalPane` wrapping `portable-pty` (mostly copy from mux)
- [ ] Verify PTY spawn, read loop, write, resize, exit handling works

#### 3b. Tab Management

- [ ] Port `Tab` (pane container with layout tree)
- [ ] Implement pane splitting (horizontal, vertical)
- [ ] Implement pane focus navigation

#### 3c. Session Management

- [ ] Port `Mux` as `Session` (top-level container)
- [ ] Implement tab creation / closing
- [ ] Implement activity tracking

#### 3d. Domain Abstraction

- [ ] Port `Domain` trait
- [ ] Port `LocalDomain` (spawns local PTY)
- [ ] Stub `SshDomain` for future implementation

### Verification

```bash
cargo test -p mythterm-mux

# Manual: open 3 tabs, split each horizontally, verify independent shells
```

---

## 9. Phase 4 — Rendering Engine Integration

**Goal:** Build the terminal rendering pipeline on top of Myth's engine.

### Sub-Tasks

#### 4a. Myth Engine Integration

- [ ] Initialize Myth engine from `myth-app` event loop
- [ ] Obtain `wgpu::Device` and `wgpu::Queue` from Myth's renderer
- [ ] Register custom render pass in Myth's render graph
- [ ] Handle window resize → resize terminal + render targets
- [ ] Handle DPI changes → re-rasterize glyphs

#### 4b. Glyph Atlas

- [ ] Implement `GlyphAtlas` struct (GPU texture + CPU-side packer)
- [ ] Use `guillotiere` for rectangle packing (same as WezTerm)
- [ ] Implement `get_or_rasterize(glyph_key) → AtlasRegion`
- [ ] Handle atlas overflow → evict LRU glyphs, rebuild
- [ ] Support multiple atlas pages (regular + emoji + CJK)
- [ ] Upload atlas to `wgpu::Texture` via staging buffer
- [ ] Implement atlas for images (sixel, iTerm2 images)

#### 4c. Terminal Render Pass

- [ ] Define `TerminalPass` implementing Myth's pass interface
- [ ] **Fallback**: If Myth render graph integration is too complex (see Risk Register), implement as raw wgpu render pass first, integrate into graph later
- [ ] Create WGSL vertex shader:
  ```wgsl
  struct VertexInput {
      @location(0) position: vec2<f32>,   // quad corner
      @location(1) uv: vec2<f32>,         // atlas UV
      @location(2) fg_color: vec4<f32>,   // foreground color
      @location(3) bg_color: vec4<f32>,   // background color
      @location(4) flags: u32,            // bold, italic, underline, etc.
  }
  ```
- [ ] Create WGSL fragment shader:
  ```wgsl
  @fragment
  fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
      let glyph = textureSample(atlas_texture, atlas_sampler, in.uv);
      let alpha = glyph.r; // single-channel glyph
      let color = mix(in.bg_color, in.fg_color, alpha);
      return color;
  }
  ```
- [ ] Implement background color pass (simple colored quads)
- [ ] Implement text glyph pass (textured quads with alpha blending)
- [ ] Implement cursor pass (blinking block/beam/underline)
- [ ] Implement selection highlight pass
- [ ] Implement IME composition overlay pass
- [ ] Implement sixel/image pass (textured quads)

#### 4d. Quad Generation

- [ ] Define `QuadVertex` (position, UV, fg, bg, flags)
- [ ] Implement `generate_quads(visible_lines, cursor, selection)`
- [ ] Use instance buffer for batched rendering
- [ ] Implement dirty-line tracking (only regenerate changed lines)
- [ ] Implement smooth scrolling (inter-frame interpolation)
- [ ] Handle wide characters (2-cell width glyphs)
- [ ] Handle combining characters (accent marks)
- [ ] Handle box-drawing characters (render as lines, not glyphs)

#### 4e. Compositing

- [ ] Layer terminal content as base layer
- [ ] Layer egui output on top
- [ ] Handle alpha blending between layers
- [ ] Handle background opacity / transparency (if config allows)

### Verification

```bash
cargo build --release -p mythterm-render

# Visual: open terminal, verify text renders correctly
# Visual: run `htop`, verify dynamic updates
# Visual: run `vim`, verify cursor + syntax highlighting
# perf: measure frame time, should be < 8ms (120fps) for scrolling
```

---

## 10. Phase 5 — UI Layer (egui)

**Goal:** Build all UI chrome with egui.

### Sub-Tasks

#### 5a. Terminal Widget

- [ ] Create `TerminalView` egui widget
- [ ] Render terminal texture as egui `Image`
- [ ] Handle click events → map to terminal cell position
- [ ] Handle scroll events → scroll terminal
- [ ] Handle drag events → extend selection
- [ ] Handle double-click → select word
- [ ] Handle triple-click → select line
- [ ] Implement right-click context menu (copy, paste, select all)

#### 5b. Tab Bar

- [ ] Implement tab bar widget (horizontal strip at top)
- [ ] Display tab titles (process name, cwd)
- [ ] Handle tab click → switch tab
- [ ] Handle tab close button (×)
- [ ] Handle tab drag → reorder
- [ ] Handle "+" button → new tab
- [ ] Support both "classic" and "fancy" tab bar styles
- [ ] Show activity indicators (bell, output in background tab)

#### 5c. Split Pane Layout

- [ ] Implement recursive binary split layout
- [ ] Render split dividers (draggable)
- [ ] Handle focus indication (highlight active pane border)
- [ ] Handle keyboard navigation between panes (Ctrl+Shift+Arrow)
- [ ] Handle pane zoom toggle

#### 5d. Scrollbar

- [ ] Implement scrollbar overlay (right edge)
- [ ] Show scroll position indicator
- [ ] Handle scrollbar drag → scroll
- [ ] Show position in scrollback (% from bottom)

#### 5e. Selection

- [ ] Render selection highlight via egui `Painter`
- [ ] Implement selection modes: cell, word, line, block
- [ ] Implement copy to clipboard on selection
- [ ] Implement paste from clipboard

#### 5f. Overlays

- [ ] Implement search overlay (Ctrl+Shift+F)
  - Text input + regex toggle
  - Match count display
  - Next/Previous navigation
  - Highlight all matches in terminal
- [ ] Implement command palette (Ctrl+Shift+P)
  - Fuzzy search for commands
  - Recent commands
- [ ] Implement settings panel
  - Font family / size picker
  - Color scheme picker
  - Keybinding editor
  - Live preview

#### 5g. Toast Notifications

- [ ] Integrate `wezterm-toast-notification` (already reusable via git dep) for OS-native toasts
- [ ] Add egui-based in-app toast overlay for non-OS notifications
- [ ] Show toasts for: update available, bell, errors
- [ ] Auto-dismiss with configurable timeout

### Verification

```bash
cargo build --release -p mythterm-ui

# Visual: open terminal, verify tab bar renders
# Visual: create 3 tabs, switch between them
# Visual: split pane, resize, close
# Visual: search (Ctrl+Shift+F), type query, verify highlights
```

---

## 11. Phase 6 — Input Handling

**Goal:** Wire keyboard and mouse input from winit/egui to the terminal.

### Sub-Tasks

#### 6a. Keyboard Input

- [ ] Capture keyboard events from egui/winit
- [ ] Map physical keys to logical keys (layout-independent)
- [ ] Apply modifier state (Ctrl, Alt, Shift, Super)
- [ ] Encode key as VT sequence based on terminal mode:
  - Normal mode
  - Application cursor mode
  - Application keypad mode
  - Kitty keyboard protocol (if negotiated)
- [ ] Handle Ctrl+key combinations (Ctrl+C, Ctrl+Z, etc.)
- [ ] Handle compose key / dead keys
- [ ] Handle IME input (preedit, commit)
- [ ] Handle paste (bracket paste mode)

#### 6b. Mouse Input

- [ ] Capture mouse events from egui/winit
- [ ] Map pixel position to cell coordinates
- [ ] Encode mouse events as VT sequences based on terminal mode
- [ ] Handle selection (click, drag, double-click, triple-click)
- [ ] Handle scroll wheel → VT scroll sequences or scrollback

#### 6c. Key Bindings

- [ ] Implement configurable keybinding system
- [ ] Map key combos to actions (new tab, close tab, split, etc.)
- [ ] Default bindings (WezTerm-compatible):
  - `Ctrl+Shift+T` → new tab
  - `Ctrl+Shift+W` → close tab
  - `Ctrl+Shift+D` → split horizontal
  - `Ctrl+Shift+E` → split vertical
  - `Ctrl+Shift+Arrow` → navigate panes
  - `Ctrl+Shift+F` → search
  - `Ctrl+Shift+P` → command palette
  - `Ctrl+Shift+L` → activate last tab
  - `Ctrl+Tab` / `Ctrl+Shift+Tab` → next/prev tab

### Verification

```bash
# In mythterm:
# Type "echo hello" → verify characters appear
# Press Ctrl+C → verify SIGINT
# Press arrow keys → verify cursor movement
# Use mouse to select text → verify selection works
```

---

## 12. Phase 7 — Configuration

**Goal:** TOML-based configuration with live reload.

### Sub-Tasks

- [ ] Define `Settings` TOML schema:
  ```toml
  [font]
  family = "JetBrains Mono"
  size = 13.0
  
  [colors]
  foreground = "#c0c0c0"
  background = "#1e1e1e"
  cursor = "#ffffff"
  
  [colors.ansi]
  black = "#000000"
  red = "#cd3131"
  # ... etc
  
  [cursor]
  style = "Block"  # Block | Beam | Underline
  blink = true
  
  [scrollback]
  lines = 10000
  
  [keybindings]
  # ...
  ```
- [ ] Implement config file resolution (`~/.config/mythterm/config.toml`)
- [ ] Implement `notify`-based file watcher for live reload
- [ ] Implement `arc-swap` for lock-free config reads
- [ ] Implement built-in color schemes (One Half Dark, Solarized, Gruvbox, etc.)
- [ ] Import WezTerm color scheme data (convert Lua tables to TOML-compatible format)

### Verification

```bash
# Change config.toml → verify terminal updates live
# Set font_size = 20.0 → verify text gets larger immediately
```

---

## 13. Phase 8 — Advanced Features

**Goal:** Integrate remaining WezTerm features with mythterm render/UI layer.

### Sub-Tasks

#### 8a. Image Protocols

Most image protocol support is already included via `wezterm-term`:
- **Sixel graphics** — included in `wezterm-term::terminalstate::sixel`
- **iTerm2 inline images** — included in `wezterm-term::terminalstate::iterm`
- **Kitty image protocol** — included in `wezterm-term::terminalstate::kitty` (~979 lines)

Remaining work:
- [ ] Verify image rendering works through mythterm-render pipeline
- [ ] Implement image scaling / sizing in render layer
- [ ] Implement image scrolling behavior in render layer

#### 8b. Unicode & Emoji

Most Unicode support is already included via WezTerm crates:
- **Bidirectional text** — included in `wezterm-bidi`
- **Unicode character properties** — included in `wezterm-char-props`
- **Emoji width / variation selectors** — handled by `wezterm-term`

Remaining work:
- [ ] Verify emoji rendering through the new font/rasterization pipeline
- [ ] Verify ZWJ sequences (family emoji, flags) render correctly with ab_glyph
- [ ] Verify combining characters work with rustybuzz shaping

#### 8c. Advanced Terminal Features

Already included in `wezterm-term::terminalstate`:
- **DECALN** (screen alignment test)
- **DECSCUSR** (cursor style)
- **REP** (repeat character)
- **DECSLRM** (left/right margins)
- **Window manipulation** (CSI t)
- **Title stack** (push/pop window title)
- **Terminal synchronization** (DCS begin/end)

No additional work needed — verify via vttest.

#### 8d. Shell Integration

Already included in `wezterm-term::terminalstate`:
- **OSC 133** semantic prompts
- **OSC 7** working directory tracking

Remaining work:
- [ ] Wire shell integration events to mythterm-ui for prompt detection
- [ ] Implement command duration tracking in mythterm-mux
- [ ] Implement semantic zones in scrollback UI

#### 8e. Copy/Paste

- [ ] Clipboard integration (xclip/wl-copy/pbcopy)
- [ ] OSC 52 clipboard protocol — already in `wezterm-term`, wire to platform clipboard
- [ ] Smart paste (strip trailing newlines, etc.)
- [ ] Copy on select (optional)

#### 8f. Hyperlinks

Already included in `wezterm-term::terminalstate`:
- **OSC 8** hyperlinks
- **URL detection** in `wezterm-surface`

Remaining work:
- [ ] Wire hyperlink events to mythterm-ui for click handling
- [ ] Implement URL hover preview in egui
- [ ] Implement open URL in browser (via `wezterm-open-url`)

### Verification

```bash
cargo test -p wezterm-term -- terminalstate::sixel
cargo test -p wezterm-term -- terminalstate::image

# Visual: display a sixel image in the terminal
# Visual: display an iTerm2 inline image
# Visual: display a Kitty image
# Visual: verify emoji rendering (family emoji, flags, skin tones)
# Visual: verify bidi text (Arabic, Hebrew) renders correctly
# Visual: verify hyperlinks are clickable
# Visual: verify clipboard integration (OSC 52)
# Visual: verify shell integration (OSC 133 prompt detection)
```

### Deferred (Future)

- Lua scripting engine
- SSH domains
- tmux integration
- Plugin system
- Remote multiplexing
- Serial port connections

---

## 14. Phase 9 — Platform & Packaging

### Sub-Tasks

- [ ] Linux packaging: `.deb`, `.rpm`, AppImage, Flatpak
- [ ] macOS packaging: `.app` bundle, Homebrew formula
- [ ] Windows packaging: `.msi`, `winget` manifest
- [ ] Desktop entry / `.desktop` file (Linux)
- [ ] Icon / branding
- [ ] man page
- [ ] Shell completions (bash, zsh, fish)

### Verification

```bash
# Build and test packaging for current platform
# Verify desktop entry works (Linux)
# Verify shell completions load correctly
```

---

## 15. Phase 10 — Performance & Polish

### Sub-Tasks

- [ ] Benchmark: scrolling throughput (lines/sec)
- [ ] Benchmark: startup time
- [ ] Benchmark: memory usage
- [ ] Benchmark: frame time (target: < 4ms for 240fps capable)
- [ ] Profile with `perf` / `Instruments` / `Tracy`
- [ ] Optimize: batch quad generation
- [ ] Optimize: minimize GPU texture uploads
- [ ] Optimize: use compute shaders for text rendering (stretch)
- [ ] Optimize: SIMD for cell grid operations
- [ ] Reduce memory: compact cell representation (WezTerm uses 16 bytes/cell)
- [ ] Accessibility: screen reader support
- [ ] Accessibility: high contrast mode

### Verification

```bash
cargo bench

# Verify frame time < 4ms for scrolling workload
# Verify memory usage is within acceptable bounds
# Verify startup time < 200ms
```

---

## 16. Dependency Graph

```
mythterm-bin ─────┬──────────────────────────────────┐
                  │                                   │
                  ▼                                   ▼
          mythterm-ui ──────► mythterm-mux      mythterm-config
           │  │                  │
           │  │                  │
           │  ▼                  ▼
           │ mythterm-render  mythterm-core (re-export)
           │  │                  │
           │  │                  │
           │  ▼                  ▼
           │ mythterm-font    wezterm-term, termwiz,
           │                  wezterm-cell, wezterm-surface,
           │                  portable-pty, etc. (git deps)
           │
           ▼
      myth-app, myth-render, myth-scene,
      myth-core, myth-assets, myth-resources
```

---

## 17. Testing Strategy

### Unit Tests

Each crate has its own test suite:

- `mythterm-core`: Run WezTerm's existing test suite (re-export crate, tests come from wezterm-term)
- `mythterm-font`: Shaping correctness, metrics calculation
- `mythterm-mux`: Tab/pane management, PTY lifecycle
- `mythterm-render`: Atlas packing, quad generation
- `mythterm-ui`: Widget layout, input mapping
- `mythterm-config`: TOML parsing, defaults

### Integration Tests

- Full terminal session: spawn shell → send commands → verify output
- Config reload: change settings → verify terminal updates
- Multi-tab: create/close/switch tabs
- Split pane: split/resize/close panes
- Scroll: scrollback navigation, search

### Visual Tests

- Screenshot comparison tests (optional, uses `screenshot-tests` crate)
- Manual vttest run
- Manual: `vim`, `htop`, `tmux` (as client), `git log --graph`

### Benchmarks

- Criterion benchmarks for shaping, rasterization, quad generation
- Frame time measurement for scrolling workload

---

## 18. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Myth engine API changes (beta) | High | High | Pin to specific commit, maintain local patches if needed |
| Config decoupling from mux | Medium | High | 42 `config::` refs, 5 Lua refs; trait-based interface to isolate |
| rustybuzz shaping gaps vs HarfBuzz | Medium | High | Fallback to HarfBuzz via `harfbuzz-sys` if needed |
| ab_glyph quality vs FreeType | Medium | Medium | Evaluate fontdue as alternative rasterizer |
| egui performance with large terminal | Medium | Medium | Limit egui to UI chrome only, render terminal directly |
| Myth render graph integration complexity | High | High | Start with raw wgpu pipeline, integrate into graph later |
| Cross-platform font discovery | Medium | Medium | Use fontconfig (Linux), CoreText (macOS), DirectWrite (Windows) |
| Sixel/image rendering in new pipeline | Low | Medium | Images are texture quads, should integrate naturally |
| WezTerm upstream breaking changes | Low | Low | Terminal core is stable; pin to specific commit |
