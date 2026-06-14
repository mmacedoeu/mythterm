# MythTerm Cinematic UI Roadmap

## Problem Statement

Reproducing photographed hardware aesthetics (OLED panels, glass, glow,
bevel) using egui is fighting the framework. Egui is a great UI toolkit
but a poor abstraction for cinematic display simulation. Hand-painting
borders/glows/shadows with `rect_filled` + `line_segment` is:
- Hard for AI agents to iterate on
- Doesn't match photographed realism
- Locks us into egui's paint model

## Core Insight

> The terminal is not a UI element. It's the content of a display device.

We should build:
```text
DisplayDevice { monitor_frame, glass, LCD_panel, chrome, post_fx }
  └─ TerminalTexture (sharp, readable)
```

## Phased Plan

### Phase 1 — SDF Shader-Driven Chrome (SHORT TERM)
**Goal:** Replace hand-painted egui borders/glows with WGSL SDF shaders.

**Why first:** Sweet spot for AI-assisted dev (per discussion Option 6).
GPU SDF gives infinite scaling, perfect AA, easy glow/bevel/animations.
AI agents are dramatically better at generating WGSL SDF shaders than
complex egui paint code.

**Scope:**
- WGSL SDF rounded-rect shader with params: radius, border_width,
  border_color, glow_strength, glow_color, fill_color, bevel_strength
- Single quad mesh, all visuals in fragment shader
- Per-tab SDF (active/inactive states differ by uniform)
- Test in isolation: `crates/sdf-test/` binary that renders a single
  SDF tab and we compare pixel-by-pixel to goal

**Success criteria:** Step 1 of tab-test renders the goal tab border
within 5 RGB units using a single SDF quad.

### Phase 2 — Display Material Pipeline (MEDIUM TERM)
**Goal:** Terminal text → texture → display material (LCD/OLED sim).

**Why:** Per discussion Option 3. The terminal becomes a material with:
- subpixel_layout (RGB stripe, pentile, etc.)
- response_curve (instant, slow LCD, phosphor CRT)
- backlight_uniformity
- glass_thickness, reflection_strength, bloom_strength

**Scope:**
- Render terminal text to a high-res RGBA texture
- DisplayMaterial struct with the above params
- Apply display material as a fullscreen quad in post-process
- Test in isolation: render a known text pattern, verify subpixel
  rendering, bloom, reflections

**Success criteria:** Terminal text rendered through display material
looks like a photographed monitor, not a software window.

### Phase 3 — Myth Scene Graph Chrome (MEDIUM TERM)
**Goal:** Move chrome (tabs, palette, search) from egui paint to Myth
entities. Egui becomes invisible interaction layer.

**Why:** Per discussion Option 1. This is the pragmatic first step
toward a full scene graph. We keep egui's interaction model (hover,
click, focus) but render everything in Myth.

**Scope:**
- TabBar becomes a Myth entity: mesh + material
- Material is the Phase 1 SDF shader
- Egui provides hit-testing and state
- Test in isolation: Myth scene with SDF tab + egui hit-test overlay

**Success criteria:** Tab rendering uses 1 draw call (SDF quad) instead
of 50+ egui paint calls. Visually identical or better.

### Phase 4 — Splat Chrome (LONG TERM)
**Goal:** Gaussian splats for glow, volumetric effects, ambient
atmosphere around the terminal.

**Why:** Per splat discussion. Splats are best for "everything except
text". Use them for glow fields around active elements, reflections
on glass, ambient atmosphere.

**Scope:**
- Splat renderer in Myth
- Spawn glow splats around active tabs, cursor, selections
- Glass reflection splats
- Test in isolation: splat field around a static tab, verify glow
  looks volumetric not screen-space

**Success criteria:** Active tab has volumetric glow that responds to
viewing angle and depth, not a flat screen-space bloom.

### Phase 5 — Full Display Engine (VISION)
**Goal:** Complete the display device architecture.
```text
Layer 0: Sharp terminal texture
Layer 1: Curved display mesh
Layer 2: Glass material
Layer 3: Splat glow field
Layer 4: Splat chrome
Layer 5: Environment splats
Layer 6: HDR postprocess
```

**Why:** This is the end state. The terminal is content on a physical
display device, rendered by a full scene graph with proper materials,
lighting, and post-processing.

**Success criteria:** MythTerm looks like a photographed monitor
displaying a terminal, not a software window with a terminal in it.

## Test Infrastructure

Each phase gets an isolated test binary in `crates/`:
- `sdf-test/` — Phase 1: SDF shader rendering
- `display-test/` — Phase 2: Display material pipeline
- `scene-test/` — Phase 3: Myth scene graph with egui interaction
- `splat-test/` — Phase 4: Splat chrome
- (Phase 5: full integration)

Each test binary:
- Runs as standalone wgpu window
- Takes a `--phase` and `--step` argument
- Renders the specific feature being tested
- Screenshot via same `tab-test-shot.sh` pattern
- Pixel-by-pixel comparison to goal

## Current State (v43)
- Active tab: hand-painted egui borders with gradient pattern
- Works, within 1-7 RGB units of goal
- But: 50+ paint calls per tab, hard to extend, fighting framework
- Next: Phase 1 — replace with single SDF quad
