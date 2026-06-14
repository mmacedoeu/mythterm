# MythTerm Cinematic UI — Multi-Phase Plan (v2)

> Canonical multi-phase plan for the cinematic / display-device rendering
> direction. Replaces and supersedes the 5-phase sketch in
> `cinematic-ui-roadmap.md` (kept as a short index for backwards
> compatibility). Use this document for scope, ordering, success
> criteria, and isolated-test design.

## 0. Why this plan exists

We are trying to reproduce photographed hardware aesthetics
(OLED panels, glass covers, cyan emissive borders, bevels) using
`egui` painters and `wgpu` quad meshes hand-assembled in
`mythterm-render`. That is the wrong abstraction:

- egui is a great UI toolkit and a poor display-hardware
  simulator.
- hand-painted borders/glows/shadows with `rect_filled` +
  `line_segment` are CPU-heavy, brittle, and resist iteration
  by AI agents.
- photographed realism is a *materials + lighting + postprocess*
  problem, not a *2D shape painting* problem.

The terminal is not a UI element. It is the *content* of a
physical display device.

We keep egui for what it is good at (input, state, debug,
tooltips) and we move all visual rendering into a stack of
shader-driven, scene-graph-driven, splat-driven layers that
myth can actually compute correctly. Each phase is small,
isolated, and testable on its own before any integration into
`mythterm-bin`.

## 1. North star

```text
Layer 0   Sharp terminal texture (sampled text)
Layer 1   Curved display mesh (or surface)
Layer 2   Display material (subpixel, response curve, backlight)
Layer 3   Glass cover (transmission + reflection)
Layer 4   Splat glow field (volumetric, depth-aware)
Layer 5   Splat chrome (tabs, palette as luminous objects)
Layer 6   Environment splats (HDRi via splats, optional)
Layer 7   Postprocess (HDR tonemap, bloom, grain, vignette)
```

When all seven layers are present and tuned, MythTerm stops
looking like a software window with a terminal in it and starts
looking like a photographed monitor displaying a terminal.

## 2. Selected directions

We are *not* building all of the seven options from the
discussion. The selected set, in priority order, is:

| Discussion option                | Phase  | Why picked                          |
|----------------------------------|--------|-------------------------------------|
| 6. SDF-driven chrome             | 1      | AI agents excel at WGSL SDF; cheap  |
| 1. egui for interaction only     | 2      | Pragmatic; immediate draw-call win  |
| 3. Terminal-as-material          | 3      | Core concept; terminal is a texture |
| 5. Slate/UMG retained scene      | 4      | Lets every chrome piece be a node   |
| 4. Sci-fi holographic OS         | 5      | Differentiator; uses 3D camera      |
| 6. Splat-based bloom (in splats) | 6      | Volumetric glow, depth-aware        |
| 7. Display engine                | 7      | End state; stitches the rest        |

Out of scope (explicitly):

- **Option 2 (abandon egui entirely)**: too aggressive as a
  primary direction. We *do* move chrome off egui, but egui
  remains for input/state in the long term.
- **Gaussian splats replacing glyph rendering**: bad fit;
  splats are inherently blurry, terminal text must be sharp.

## 3. Phase overview

| # | Phase                       | Test crate           | Effort   | Status     |
|---|-----------------------------|----------------------|----------|------------|
| 1 | SDF shader-driven chrome    | `sdf-test`           | XS–S     | In flight  |
| 2 | Myth scene-graph chrome     | `scene-chrome-test`  | S        | Not started|
| 3 | Display material pipeline   | `display-test`       | M        | Not started|
| 4 | Retained scene graph        | `retained-scene-test`| M        | Not started|
| 5 | Holographic terminal        | `hologram-test`      | L        | Not started|
| 6 | Splat glow field            | `splat-test`         | M        | Not started|
| 7 | Display engine integration  | (the main bin)       | L        | Not started|

Each phase has an isolated test binary under `crates/`. Each
test binary renders *one thing*, takes a `--step` flag, and
emits a PNG to a known path so we can diff against a goal
image. No test binary depends on `mythterm-core`, `mythterm-mux`,
or the main render chain.

## 4. Phase 1 — SDF shader-driven chrome (in flight)

**Source:** discussion option 6. **Test crate:** `sdf-test`.

**Goal:** replace hand-painted `egui` borders/glows with a
single WGSL SDF shader. The shape, fill, border, glow, bevel,
and animations are all computed in the fragment shader on a
single quad.

**Why first:** AI agents are dramatically better at writing
WGSL SDF shaders than at writing complex egui paint code. The
shader is portable, scalable, easy to animate, and one draw
call replaces 50+.

**Scope (already partially done in `sdf-test/src/main.rs`):**

- WGSL `sd_rounded_box` SDF for a generic rounded rectangle.
- Per-border parameters: `corner_radius`, `border_width`,
  `border_color`, `glow_strength`, `glow_color`, `fill_color`,
  `bevel_strength`.
- Per-tab uniforms: `SdfParams` struct, 16-byte aligned.
- Active vs inactive states differ by uniform.
- Single quad mesh, all visuals in the fragment shader.

**Steps (already in code, see `sdf-test`):**

- `--step 1`: solid fill (SDF validation)
- `--step 2`: fill + border
- `--step 3`: fill + border + glow
- `--step 4`: full active-tab style (navy fill, cyan border,
  horizontal brightness gradient)

**Success criteria:**

- `sdf-test --step=4` renders within 5 RGB units of the goal
  tab at 800×600.
- One draw call per tab.
- Uniforms are exactly 16-byte aligned, no padding warnings.
- `corners == sharp` (the goal tab has `corner_radius == 0`).

**Risks:**

- WGSL uniform layout can drift. Mitigated by `array<vec4,
  11>` and accessors on both sides.
- Gradient asymmetry is hard to express in pure SDF. Mitigated
  by a `peak_at(t, color)` helper that takes the peak position
  and bell-falls on both sides.

## 5. Phase 2 — Myth scene-graph chrome

**Source:** discussion option 1. **Test crate:**
`scene-chrome-test` (new).

**Goal:** make chrome (tabs, palette, search) a *Myth entity*
instead of an egui paint job. Egui becomes an invisible
interaction layer that only emits hover/click/focus state.

**Why:** one of the most pragmatic wins. We keep egui's
interaction model but reduce the per-tab paint cost from 50+
egui calls to 1 quad with a bound SDF material. This is the
first step toward a full scene graph.

**Scope:**

- `TabBar` becomes a myth entity: `Mesh = quad`,
  `Material = SdfMaterial` (Phase 1's WGSL).
- Myth scene: `Node { transform, mesh, material }`.
- Egui: a `Sense::hover()` rect per tab that emits
  `(tab_id, Hovered | Clicked | FocusGained)`.
- Render order: `Myth(chrome) → egui(input) → Myth(input
  cursor)`.
- Test binary renders a Myth scene with 5 tabs (1 active, 4
  inactive) and overlays invisible egui hit zones.

**Steps:**

1. `scene-chrome-test --step=1`: single quad, single tab, no
   interaction.
2. `scene-chrome-test --step=2`: 5 tabs, alternating active
   state, no interaction.
3. `scene-chrome-test --step=3`: + egui hit overlay, hover
   state changes a uniform (`hover_strength: f32`).
4. `scene-chrome-test --step=4`: + click-to-activate, focus
   ring, animations.

**Success criteria:**

- 1 draw call per tab.
- Visual output is identical to Phase 1 (same shader).
- Hover state visibly changes the glow uniform within 16 ms.
- Click-to-activate round-trips through egui → myth uniform
  → next frame.

**Risks:**

- Myth scene + egui compositing can race. We render egui
  *after* the chrome pass, with `BlendState::ALPHA_BLENDING`,
  to avoid z-fighting.
- Myth `0.2` API has changed since 0.1. The Myth 0.2 scene
  graph uses `Node` + `Mesh` + `Material` handles.

## 6. Phase 3 — Display material pipeline

**Source:** discussion option 3. **Test crate:**
`display-test` (new).

**Goal:** the terminal text becomes a *display material*. The
material has its own subpixel layout, response curve, glass
thickness, backlight intensity, and bloom strength. The
terminal is no longer a 2D image — it is a simulated
display technology applied to a quad.

**Why:** this is the core conceptual shift. Once the terminal
is a *material*, the difference between a "monitor" and a
"hologram" or "OLED panel" is just a parameter struct on the
material, not a new renderer.

**Scope:**

- Render terminal text to a high-res RGBA texture (sharp,
  MSDF or SDF glyphs).
- `DisplayMaterial` struct with: `subpixel_layout`
  (`RgbStripe`, `Pentile`, `Diamond`), `response_curve`
  (`Instant`, `Lcd8ms`, `PhosphorP22`), `backlight_uniformity`,
  `glass_thickness`, `reflection_strength`, `bloom_strength`.
- Apply display material as a fullscreen quad in post-process.
- Test binary: known text pattern (e.g. "MythTerm v2 Phase 3
  Display Material") rendered through each preset.

**Steps:**

1. `display-test --step=1`: terminal text → RGBA texture, no
   material.
2. `display-test --step=2`: + subpixel sampling (RGB stripe).
3. `display-test --step=3`: + LCD response curve (temporal
   buffer, requires `fence`/`timestamp`).
4. `display-test --step=4`: + glass reflection (procedural
   cubemap or splat field).
5. `display-test --step=5`: + backlight uniformity (radial
   gradient in shader).
6. `display-test --step=6`: full DisplayMaterial, all
   parameters in one struct, hot-swappable via uniform.

**Success criteria:**

- The same terminal text looks materially different under
  `Lcd8ms + RgbStripe` vs `Pentile + PhosphorP22`.
- Subpixel rendering survives a 1× and 2× DPI scale without
  aliasing.
- Glass reflection is visible but never obscures text
  (`intensity < 0.2`).

**Risks:**

- Temporal response curves need a *history* texture; adds
  memory but is well-bounded.
- Procedural cubemap is cheaper than a HDRi; we already have
  one in `mythterm-render::environment` — reuse it.

## 7. Phase 4 — Retained scene graph

**Source:** discussion option 5. **Test crate:**
`retained-scene-test` (new).

**Goal:** every visible thing is a *Node* in a retained scene
graph. Animations are *Components*. State is *Data* on the
Node. The renderer walks the graph and emits draw calls.

**Why:** this is how Unreal (`UMG`), Unity (`UI Toolkit`),
Figma, and modern design tools structure UI. It composes
cleanly with myth's existing scene graph. It makes animations
trivial (change a uniform on a Node, the renderer picks it
up next frame). It makes the SDF chrome, the display material,
and the splat glow all *just nodes*.

**Scope:**

- `enum Node { Quad { mesh, material }, Splat { gaussians },
  Text { content, font }, Group { children } }`.
- `Component`: `Transform`, `Animation { from, to, easing,
  duration_ms }`, `Hover`, `Focus`, `Clickable { on_click }`.
- `Scene` is the root `Group`. Render walks it, sorts by
  material, batches.
- `Tab { id, label, state }` is a `Group` containing a
  `Quad` (the background) and a `Text` (the label).
- Test binary: 5 tabs, with hover/focus animations, splat
  glow attached to active tab.

**Steps:**

1. `retained-scene-test --step=1`: flat list of `Quad`
   nodes, manual transform.
2. `retained-scene-test --step=2`: nested `Group`, scene
   walks children.
3. `retained-scene-test --step=3`: `Tab` group with hover
   animation.
4. `retained-scene-test --step=4`: `Splat` node attached to
   active tab.

**Success criteria:**

- A scene with 5 tabs, 1 active, 4 inactive renders in 1
  frame budget (16 ms) at 1080p.
- Hover animation: 0 → 1 over 200 ms with `ease_out_cubic`.
- Adding a new tab is *one* `Scene::add(Tab { ... })` call;
  the renderer picks it up next frame.

**Risks:**

- The retained model is a *big* refactor of `mythterm-ui`
  and `mythterm-render`. The mitigation is that the test
  crate *is* the prototype. Integration only starts once the
  test crate proves the model.

## 8. Phase 5 — Holographic terminal

**Source:** discussion option 4. **Test crate:**
`hologram-test` (new).

**Goal:** the terminal exists in *world space*. A 3D camera
sees a curved display mesh. Floating widgets orbit. Particles
drift. The terminal is still readable because text is rendered
on a flat plane attached to the mesh.

**Why:** this is the differentiator. Once a real camera and a
real curved mesh exist, "monitor", "tablet", and "hologram"
are just camera distances. Splats become natural for
volumetric effects around the mesh.

**Scope:**

- `Camera { position, target, fov, near, far }`.
- `DisplayMesh`: a `PlaneGeometry` with a `curvature` field
  that bends the mesh (vertex shader does barrel/bend).
- The terminal texture is sampled *on* the mesh's UVs.
- Widgets (tabs, palette) are `Node`s positioned in 3D space.
- Particles: `Splat` clouds with a `drift` animation.

**Steps:**

1. `hologram-test --step=1`: flat plane, orthographic camera,
   terminal texture sampled.
2. `hologram-test --step=2`: + curvature (vertex shader).
3. `hologram-test --step=3`: + perspective camera, mouse
   orbit.
4. `hologram-test --step=4`: + 3D-positioned chrome.
5. `hologram-test --step=5`: + particle splat field.

**Success criteria:**

- Curved mesh samples the terminal texture with sub-pixel
  accuracy.
- Camera orbit is smooth, no jitter.
- Tabs positioned in 3D are still hit-testable from a 2D
  mouse position (ray-cast).

**Risks:**

- Text on a curved surface is the hardest readability
  problem. We may need a *flat text layer* and a *curved
  scene layer* composited in post.
- Mouse-to-3D-ray hit-testing is non-trivial. We will use
  myth's existing ray cast.

## 9. Phase 6 — Splat glow field

**Source:** splat discussion option 6. **Test crate:**
`splat-test` (new).

**Goal:** Gaussian splats provide *volumetric, depth-aware*
glow around bright elements (active tab, cursor, selection).
Splats are *not* used for glyphs.

**Why:** screen-space bloom flattens depth. Splat glow is
volumetric, responds to camera, and never aliases.

**Scope:**

- A `SplatField { gaussians: Vec<Gaussian> }` where
  `Gaussian { pos, scale, color, opacity }`.
- A wgpu pipeline that sorts gaussians and renders them as
  billboard quads with a Gaussian falloff shader.
- Splats are *spawned* by events: tab becomes active →
  20–50 splats emit from tab bounds over 200 ms; cursor
  blinks → 5–10 splats emit per blink.
- Test binary: static splat field around a static tab, then
  animated emission around an active tab.

**Steps:**

1. `splat-test --step=1`: 1000 splats, manual positions,
   render.
2. `splat-test --step=2`: emissive event spawns splats
   around an active tab.
3. `splat-test --step=3`: splat field reacts to camera
   (perspective distortion visible).
4. `splat-test --step=4`: + depth-aware occlusion (splat
   behind the terminal mesh is hidden).

**Success criteria:**

- Active tab has a glow that *feels volumetric*, not
  screen-space.
- Glow intensity scales with `1/distance^2` (Gaussian).
- Performance: 50k splats at 60 fps on RTX 2060.

**Risks:**

- Splat sorting is O(n log n) per frame. We amortize with a
  spatial hash if needed.
- wgpu's instanced rendering is the right tool here. We do
  *not* need a custom renderer.

## 10. Phase 7 — Display engine integration

**Source:** discussion option 7. **Test target:**
`mythterm-bin` (the real binary).

**Goal:** assemble all prior phases into the production
render path. MythTerm becomes:

```text
Terminal Core (wezterm-term)
        ↓
Terminal Texture (sharp RGBA, MSDF glyphs)
        ↓
Display Mesh (curved plane)
        ↓
Display Material (subpixel, response, backlight, glass)
        ↓
Splat Glow Field
        ↓
Splat Chrome
        ↓
Environment Splats
        ↓
Postprocess (HDR tonemap, bloom, grain, vignette)
        ↓
Swapchain
```

**Why:** this is the end state. The terminal is content on a
physical display device, rendered by a full scene graph.

**Scope:**

- Refactor `mythterm-render` to expose a `DisplayEngine`
  that owns the 8 layers.
- Refactor `mythterm-ui` to emit `Node` updates instead of
  egui shapes for chrome.
- Keep `mythterm-core`, `mythterm-mux`, `mythterm-config`
  untouched.
- The egui render path is reduced to *debug + input + state
  readouts only*.

**Steps:**

1. Refactor `mythterm-render` to expose `DisplayEngine`.
2. Add a `display_engine: bool` feature flag; off = current
   behavior, on = new path.
3. Run both paths in parallel during dev. Diff screenshots.
4. Flip the feature flag. Remove the old path.
5. Update `PLAN.md` Phase 5 ("UI Layer") to point here.

**Success criteria:**

- A user looking at MythTerm cannot tell whether it is a
  software window or a photographed monitor. (Subjective
  but verifiable by side-by-side with a real OLED photo.)
- Frame budget held at 16 ms (60 fps) at 1080p.
- The chrome is 90% fewer draw calls than the egui version.

**Risks:**

- This is a *big* refactor. The mitigation is that every
  layer was prototyped in isolation first.
- Performance regressions in myth 0.2 will be the main
  blocker; we have a 0.2 release pinned.

## 11. Cross-cutting concerns

### 11.1 Test infrastructure

Each phase's test binary must:

- Run as a standalone wgpu window (no `mythterm-bin`
  dependencies).
- Accept `--step N` (or `--phase N --step M`).
- Render *one* thing in isolation.
- Write a PNG to `target/snapshots/<crate>_<step>.png` on
  exit (or on demand via a key press).
- Have a `compare.sh` (or just instructions) to diff the
  PNG against a goal image.

A shared `crates/test-harness` crate provides:

- `WindowedApp`: winit + wgpu + surface lifecycle.
- `Snapshot::write(path)`: copies the swapchain to a PNG.
- `PngDiff::max_rgb_diff(a, b)`: 0..255, for "within 5 RGB
  units" checks.

### 11.2 Goal images

Each test binary has a *goal image* checked into
`crates/<crate>/goal/<step>.png`. Success is *visually
within N RGB units* of the goal. We accept 5 RGB units for
shape reproduction, 10 RGB units for animations, 20 RGB
units for splat glow (which is inherently noisy).

### 11.3 AI-agent iteration loop

The SDF-heavy phases (1, 2, 4) are designed to be
AI-agent-friendly:

- One WGSL string per change.
- One uniform struct per change.
- One PNG output per change.
- A diff to the goal image is the success signal.

This is the explicit pitch from the discussion: AI agents
are dramatically better at generating WGSL SDF shaders than
at generating complex egui paint code.

### 11.4 Dependencies between phases

```text
1 (SDF chrome)
  ↓
2 (Myth scene chrome)  ←── uses Phase 1's shader
  ↓
3 (Display material)    ←── independent of 2; could be parallel
  ↓
4 (Retained scene)      ←── uses Phase 2's chrome + Phase 6's splats
  ↓
5 (Hologram)            ←── uses Phase 4's retained scene
  ↓
6 (Splat glow)          ←── could start in parallel with Phase 2
  ↓
7 (Integration)         ←── uses everything
```

Phases 2, 3, 6 are good candidates to be worked on in
parallel by different agents.

## 12. What we are explicitly *not* doing

- Replacing the terminal text with splats. (Blurry. Bad.)
- Fully abandoning egui. (Useful for state, input, debug.)
- Re-writing wezterm's PTY/mux. (Already done via
  `mythterm-mux`.)
- Adding a HDRi loader this cycle. (We have a procedural
  cubemap; a real HDRi can land later.)
- Writing a custom 2D engine. (Myth already is one.)

## 13. Per-phase checklist (compact)

- [ ] **P1 SDF chrome** — `sdf-test --step=4` matches goal
- [ ] **P2 Myth scene chrome** — 5 tabs, 1 draw call each
- [ ] **P3 Display material** — same text, 4 distinct
  material presets
- [ ] **P4 Retained scene** — Tab group, hover animation
- [ ] **P5 Hologram** — curved mesh, perspective camera
- [ ] **P6 Splat glow** — volumetric, depth-aware
- [ ] **P7 Integration** — feature flag flipped, draw calls
  down 90%

## 14. Open questions

- Myth 0.2 scene-graph API is still settling; we may need
  to pin a commit.
- Splat sort cost on Linux/Intel iGPU — unmeasured.
- Whether the holographic camera should be opt-in or the
  default. (Suggest: opt-in via config until Phase 7
  proves the visual quality.)
- MSDF vs SDF glyphs for the terminal texture. (MSDF is
  sharper at small sizes; SDF is simpler.)
