#!/usr/bin/env bash
# test_step.sh — run a single test step, snapshot, and diff.
#
# Usage:
#   tools/test_step.sh <crate> <step> [goal]
#
# Example:
#   tools/test_step.sh sdf-test 4
#   tools/test_step.sh sdf-test 4 crates/sdf-test/goal/4.png
#
# What it does:
#   1. Builds <crate>'s binary in release mode.
#   2. Runs it in the background with --step=<step>, capturing
#      a PNG to target/snapshots/<crate>_<step>.png.
#      (The binary is expected to write the PNG itself; see
#      docs/cinematic-ui-plan.md § 11 for the convention.)
#   3. Diffs the snapshot against <goal> (default:
#      crates/<crate>/goal/<step>.png) via compare_png.py.
#
# Exit code 0 = pass, 1 = diff fail, 2 = build/run error.

set -euo pipefail

crate="${1:?usage: $0 <crate> <step> [goal]}"
step="${2:?usage: $0 <crate> <step> [goal]}"
goal="${3:-crates/${crate}/goal/${step}.png}"

snapshot_dir="target/snapshots"
snapshot="${snapshot_dir}/${crate}_${step}.png"
mkdir -p "${snapshot_dir}"

echo "==> building ${crate}"
cargo build -p "${crate}" --release

# The test binary writes a PNG to ${snapshot} on key press or
# on exit. We run it for a few seconds and kill it. The exact
# mechanism will be implemented per crate — see the canonical
# plan § 11.
echo "==> running ${crate} --step=${step} (writes ${snapshot})"
cargo run -p "${crate}" --release -- --step="${step}" &
pid=$!
trap "kill ${pid} 2>/dev/null || true" EXIT
# Wait for the snapshot to appear, up to 10 s.
for i in $(seq 1 100); do
    if [[ -f "${snapshot}" ]]; then
        break
    fi
    sleep 0.1
done
kill "${pid}" 2>/dev/null || true
wait "${pid}" 2>/dev/null || true

if [[ ! -f "${snapshot}" ]]; then
    echo "FAIL: no snapshot produced at ${snapshot}"
    exit 2
fi

echo "==> diffing against ${goal}"
python3 tools/compare_png.py "${goal}" "${snapshot}" \
    --diff-out "${snapshot_dir}/${crate}_${step}_diff.png"
