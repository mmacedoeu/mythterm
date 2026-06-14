# scene-chrome-test (Phase 2 prototype)

> Phase 2 of [`docs/cinematic-ui-plan.md`](../../docs/cinematic-ui-plan.md):
> **Myth scene-graph chrome**. Each tab is a *Node* — a quad mesh
> + an SDF material with its own uniform buffer. Egui is used
> only as an invisible input pump for hit-testing (Steps 3+).

## What it proves

- 5 tabs, 1 draw call per tab, no egui paint calls.
- 1 retained uniform buffer per tab, mutated per-frame on hover/click.
- 1 pipeline + 1 vertex buffer, shared by all tabs.
- Snapshot path that round-trips a frame through an offscreen
  `Rgba8Unorm` texture and writes a PNG (no sRGB re-encode).

## Run

```bash
cargo run -p scene-chrome-test --release -- --step=2
```

Key bindings (interactive only):
- `S` — snapshot current frame
- `1..4` — jump to step
- `Q` / `Esc` — quit

Headless / CI:
```bash
./target/release/scene-chrome-test --step=2 --snapshot-at=3
```

## Steps

| Step | What it demonstrates                                        | Goal                       |
|------|-------------------------------------------------------------|----------------------------|
| 1    | Single tab quad, no interaction                             | `goal/1.png`               |
| 2    | Five tabs (1 active, 4 inactive) from a `Vec<SdfNode>`      | `goal/2.png`               |
| 3    | + egui input pump; hover brightens glow uniform             | `goal/3.png` (static only) |
| 4    | + click-to-activate, focus ring, animated transition        | `goal/4.png` (static only) |

Steps 3 and 4 are visually identical to step 2 when no mouse
interaction is provided. The difference is interaction
behavior; CI only validates steps 1 and 2 headlessly.

## Diff

```bash
python3 ../../tools/compare_png.py goal/2.png \
    ../../target/snapshots/scene-chrome-test_2.png \
    --max 5 --thresh 5 --px-frac 0.01
```

Current status: `max_rgb_diff = 0` for all 4 steps.

## File map

| File                       | Role                                                |
|----------------------------|-----------------------------------------------------|
| `src/main.rs`              | The whole prototype (self-contained).               |
| `goal/{1,2,3,4}.png`       | Reference outputs for `--step=N` runs.              |
| `../../tools/compare_png.py` | Diff tool (shared with `sdf-test`).                |
| `../../tools/test_step.sh`  | Build + run + snapshot + diff wrapper (shared).     |

## What this crate is *not*

- It does not use the Myth engine yet. The scene graph here
  is a small Rust struct. Switching to `myth_render` /
  `myth_scene` is the next step (Phase 2 → Phase 4).
- It does not yet handle window resize of the *scene* (only
  the swapchain is reconfigured).
- It does not animate state changes — hover/click are
  instantaneous (focus/hover `+= (target - x) * 0.15` in
  `update_node_params` is the start of a smooth animation).
