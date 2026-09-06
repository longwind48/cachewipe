#!/usr/bin/env bash
# Build a deterministic fake $HOME for cachewipe evals.
#
# Every eval run gets its OWN copy, because --apply mutates the tree. Sizes are
# small but distinct so "largest first" ordering is meaningful and so a
# double-counted total is visible as a number that exceeds the real bytes.
#
# Usage: make-fixture.sh /path/to/fixture-home
set -euo pipefail
DEST="${1:?usage: make-fixture.sh <dest>}"
rm -rf "$DEST"
mkdir -p "$DEST"

# fill <path> <MB>
fill() {
  mkdir -p "$(dirname "$1")"
  dd if=/dev/zero of="$1" bs=1m count="$2" status=none
}

# --- package caches, incl. the macOS-only locations the old catalog missed ---
fill "$DEST/.cache/uv/archive/wheel.bin" 8
fill "$DEST/Library/pnpm/store/v3/files/00/blob.bin" 12      # macOS pnpm store
fill "$DEST/Library/Caches/pip/http/ab/cd/blob.bin" 5        # macOS pip cache
fill "$DEST/.npm/_cacache/content-v2/blob.bin" 2
fill "$DEST/.npm/_npx/0b9ff77863cb6e9f/node_modules/pkg/i.js" 4

# PNPM_HOME also holds the pnpm binary — must never be a target, only the store.
fill "$DEST/Library/pnpm/pnpm" 1

# --- versioned browser drivers: two products, two builds each ---
for v in chromium-1100 chromium-1200 firefox-1400 firefox-1500; do
  fill "$DEST/Library/Caches/ms-playwright/$v/browser.bin" 3
  sleep 0.05   # make the higher build number the newer mtime
done

# --- container VM disks (the biggest thing here, as on a real machine) ---
fill "$DEST/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw" 40
fill "$DEST/.finch/.disks/d0791de808fd746d" 25

# --- projects: build artifacts, the target/ trap, and a nesting case ---
# 1. A plain DATA dir named target/. No manifest. Nothing regenerates this.
fill "$DEST/projects/etl/target/2026-01-output.parquet" 6
fill "$DEST/projects/etl/source/input.csv" 1

# 2. A real Rust project, proven by the sibling Cargo.toml.
echo '[package]
name = "tool"' > /dev/null
mkdir -p "$DEST/projects/rustapp"
printf '[package]\nname = "tool"\n' > "$DEST/projects/rustapp/Cargo.toml"
fill "$DEST/projects/rustapp/target/debug/binary" 7

# 3. node_modules nested inside .next — the cross-target double-count case.
fill "$DEST/projects/web/.next/standalone/frontend/node_modules/dep/i.js" 5
fill "$DEST/projects/web/.next/build-manifest.json" 1
fill "$DEST/projects/web/node_modules/react/index.js" 4
printf '{"name":"web"}\n' > "$DEST/projects/web/package.json"

# 4. __pycache__ inside an already-matched .venv — same double-count class.
fill "$DEST/projects/api/.venv/lib/python3.12/site-packages/pandas/__pycache__/x.pyc" 3
fill "$DEST/projects/api/.venv/bin/python" 2
fill "$DEST/projects/api/src/__pycache__/main.pyc" 1

echo "fixture at $DEST"
du -sh "$DEST"
