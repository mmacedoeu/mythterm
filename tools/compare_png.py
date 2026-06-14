#!/usr/bin/env python3
"""
compare_png.py — visual diff between an actual PNG and a goal PNG.

Reports:
  - max_rgb_diff   : the largest per-channel absolute difference
                     across every pixel.
  - mean_rgb_diff  : the mean per-channel absolute difference.
  - px_over_thresh : the fraction of pixels whose max-channel
                     difference exceeds `thresh`.

Exits 0 if max_rgb_diff <= max_ok, else 1. Designed to be called
from cargo test or a shell loop:

    python3 tools/compare_png.py goal.png actual.png \
        --max 5 --thresh 5 --px-frac 0.01

The default thresholds match the success criteria in
docs/cinematic-ui-plan.md (5 RGB units for shapes, 10 for
animations, 20 for splats).

Requires Pillow. Install with `pip install pillow` if missing.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.stderr.write(
        "compare_png.py: Pillow not found. Install with `pip install pillow`.\n"
    )
    sys.exit(2)


def diff(goal: Path, actual: Path) -> tuple[Image.Image, Image.Image]:
    g = Image.open(goal).convert("RGB")
    a = Image.open(actual).convert("RGB")
    if g.size != a.size:
        sys.stderr.write(
            f"size mismatch: goal {g.size} != actual {a.size}\n"
        )
        sys.exit(2)
    return g, a


def stats(g: Image.Image, a: Image.Image, thresh: int) -> tuple[int, float, float]:
    """Return (max_rgb_diff, mean_rgb_diff, px_over_thresh)."""
    gp = g.load()
    ap = a.load()
    w, h = g.size
    total = w * h
    if total == 0:
        return 0, 0.0, 0.0
    max_d = 0
    sum_d = 0
    over = 0
    # Channel-wise absolute diff. We loop in Python because we
    # want a single, dependency-light pass; the test images are
    # 800x600 (480k pixels) so this is well under a second.
    for y in range(h):
        for x in range(w):
            gr, gg, gb = gp[x, y]
            ar, ag, ab = ap[x, y]
            dr = abs(gr - ar)
            dg = abs(gg - ag)
            db = abs(gb - ab)
            m = max(dr, dg, db)
            if m > max_d:
                max_d = m
            sum_d += dr + dg + db
            if m > thresh:
                over += 1
    mean_d = sum_d / (3.0 * total)
    px_frac = over / total
    return max_d, mean_d, px_frac


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("goal", type=Path)
    p.add_argument("actual", type=Path)
    p.add_argument("--max", type=int, default=5,
                   help="max RGB diff to pass (default: 5)")
    p.add_argument("--thresh", type=int, default=5,
                   help="per-pixel 'over' threshold (default: 5)")
    p.add_argument("--px-frac", type=float, default=0.01,
                   help="max fraction of pixels over --thresh "
                        "(default: 0.01 = 1%%)")
    p.add_argument("--diff-out", type=Path, default=None,
                   help="optional: write a visual diff PNG to this path")
    args = p.parse_args()

    if not args.goal.exists():
        sys.stderr.write(f"goal not found: {args.goal}\n")
        return 2
    if not args.actual.exists():
        sys.stderr.write(f"actual not found: {args.actual}\n")
        return 2

    g, a = diff(args.goal, args.actual)
    max_d, mean_d, px_frac = stats(g, a, args.thresh)

    print(f"max_rgb_diff  = {max_d}")
    print(f"mean_rgb_diff = {mean_d:.3f}")
    print(f"px_over_{args.thresh} = {px_frac * 100:.3f}%")

    if args.diff_out is not None:
        # Brighten the diff so it is visible.
        from PIL import ImageChops
        d = ImageChops.difference(g, a)
        d = d.point(lambda v: min(255, v * 8))
        d.save(args.diff_out)

    if max_d > args.max:
        print(f"FAIL: max_rgb_diff {max_d} > --max {args.max}")
        return 1
    if px_frac > args.px_frac:
        print(f"FAIL: px_over_thresh {px_frac:.4f} > --px-frac {args.px_frac}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
