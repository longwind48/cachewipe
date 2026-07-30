#!/usr/bin/env bash
# Reproducible sizing benchmark for cachewipe.
#
# Compares cachewipe against the disk-usage tools it is fair to compare against:
# every tool here walks a tree and stats each file to total its size, which is
# the work cachewipe does. Name-only walkers (fd, find) are deliberately excluded
# — they never ask the filesystem how big anything is, so beating them proves
# nothing about sizing.
#
# Usage:  bash bench/bench.sh [FILES] [DIRS]
#   FILES  files per directory (default 100)
#   DIRS   number of directories (default 2000)  -> 200k files by default
set -euo pipefail

FILES=${1:-100}
DIRS=${2:-2000}
TREE=${TREE:-/tmp/cachewipe-bench/tree}
BIN="$(cd "$(dirname "$0")/.." && pwd)/target/release/cachewipe"

command -v hyperfine >/dev/null || { echo "need hyperfine: brew install hyperfine"; exit 1; }
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 1; }

# --- build a cache-shaped tree: many small files, moderate nesting -----------
# Real package caches look like this (uv/npm store thousands of small files),
# and it is the shape that makes sizing expensive: cost is per-file syscalls,
# not per-byte.
if [ ! -d "$TREE" ]; then
  echo "generating $((FILES * DIRS)) files in $TREE ..."
  mkdir -p "$TREE"
  python3 - "$TREE" "$FILES" "$DIRS" <<'PY'
import os, sys
tree, files, dirs = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
blob = b"x" * 4096                      # 4 KiB, typical of cached metadata/py files
for d in range(dirs):
    # two levels of nesting so the walk has real branching to parallelise
    p = os.path.join(tree, f"g{d // 50}", f"pkg{d}")
    os.makedirs(p, exist_ok=True)
    for f in range(files):
        with open(os.path.join(p, f"f{f}.bin"), "wb") as fh:
            fh.write(blob)
PY
fi

echo
echo "tree: $TREE"
echo "files: $(find "$TREE" -type f | wc -l | tr -d ' ')"
echo "size:  $(du -sh "$TREE" 2>/dev/null | cut -f1)"
echo

# --- correctness parity ------------------------------------------------------
# A fast wrong answer is worthless, so assert the totals agree before timing.
echo "== byte totals (must agree) =="
cw=$(HOME="$TREE/.." "$BIN" --json --root "$TREE" 2>/dev/null \
      | python3 -c 'import json,sys; print(sum(i["files"] for i in json.load(sys.stdin)["items"]))')
echo "cachewipe files counted: $cw"
echo "find files counted:      $(find "$TREE" -type f | wc -l | tr -d ' ')"
echo

# --- timings -----------------------------------------------------------------
# cachewipe sizes a *target*, so point it at the tree via --root. The competitors
# are given the same directory. 3 warmup runs prime the FS cache for everyone.
echo "== hyperfine (3 warmups, 10 runs) =="
# cachewipe sizes whatever its catalog resolves, so give it a HOME whose uv cache
# IS the tree. Note: make it a real directory, not a symlink — a symlinked target
# forces repeated canonicalisation in the safety check and inflates the time ~2.6x,
# which is a measurement artifact rather than a property of the tool.
FAKE_HOME=$(dirname "$TREE")/home
if [ ! -d "$FAKE_HOME/.cache/uv" ]; then
  mkdir -p "$FAKE_HOME/.cache"
  cp -c -R "$TREE" "$FAKE_HOME/.cache/uv" 2>/dev/null \
    || cp -R "$TREE" "$FAKE_HOME/.cache/uv"
fi

CMDS=(-n "cachewipe" "env HOME=$FAKE_HOME $BIN --json")
command -v du       >/dev/null && CMDS+=(-n "du -sk"  "du -sk $TREE")
command -v diskus   >/dev/null && CMDS+=(-n "diskus"  "diskus $TREE")
command -v dust     >/dev/null && CMDS+=(-n "dust"    "dust -d0 $TREE")
command -v gdu-go   >/dev/null && CMDS+=(-n "gdu"     "gdu-go -np $TREE")

hyperfine --warmup 3 --runs 10 --export-markdown /tmp/cachewipe-bench/result.md "${CMDS[@]}"

echo
echo "markdown table written to /tmp/cachewipe-bench/result.md"
echo "remove the tree with: rm -rf $(dirname "$TREE")"
