# MythTerm Cinematic UI Roadmap (index)

> **The canonical, multi-phase plan lives in
> [`cinematic-ui-plan.md`](./cinematic-ui-plan.md).** This file is
> kept as a short, single-page index for backwards compatibility.
> Read the canonical plan for scope, ordering, success criteria,
> and isolated test design.

## Phase summary

| # | Phase                       | Test crate             | Status     |
|---|-----------------------------|------------------------|------------|
| 1 | SDF shader-driven chrome    | `sdf-test`             | Done       |
| 2 | Myth scene-graph chrome     | `scene-chrome-test`    | Done       |
| 3 | Display material pipeline   | `display-test`         | Done       |
| 4 | Retained scene graph        | `retained-scene-test`  | Not started|
| 5 | Holographic terminal        | `hologram-test`        | Not started|
| 6 | Splat glow field            | `splat-test`           | Not started|
| 7 | Display engine integration  | `mythterm-bin`         | Not started|

See `cinematic-ui-plan.md` for the full description of each
phase, including the WGSL / Rust / Myth surface area, the
isolated test crate design, and the success criteria.

## Selected directions (from the design discussion)

- Discussion option 6 (SDF-driven chrome) → Phase 1
- Discussion option 1 (egui for interaction only) → Phase 2
- Discussion option 3 (terminal-as-material) → Phase 3
- Discussion option 5 (retained scene / UMG-style) → Phase 4
- Discussion option 4 (sci-fi holographic OS) → Phase 5
- Splat option 6 (splat-based bloom) → Phase 6
- Discussion option 7 (display engine architecture) → Phase 7

## Test infrastructure

Each phase's test binary lives at `crates/<name>/`. Each one
runs as a standalone wgpu window, accepts a `--step` flag, and
is expected to render *one* thing in isolation before
integration.

Goal images and the diff script live in the canonical plan's
section 11.

## Current state (v45)

- Phases 1, 2, 3 are complete. All 12 snapshots
  (sdf-test 1–2, scene-chrome-test 1–4, display-test 1–6) are
  byte-perfect against their goal PNGs in `crates/<name>/goal/`.
- `display-test` is the most recent: a unified
  `DisplayMaterial` struct (subpixel + LCD response + glass +
  backlight + bloom) parameterised by a single uniform, with
  number-key 1..6 hot-swap at runtime.
- The test binaries share `wgsl-sdf::png::write_png_rgba` for
  snapshot output, so the PNG pipeline is one crate-wide
  dependency.
