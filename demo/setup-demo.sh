#!/bin/bash
# Seeds a sandbox HOME with fake caches so demo.tape can record a real
# cachewipe run without touching anything of the user's.
# The binary and its logic are real; only the cache contents are synthetic.
set -eu

DEMO_HOME="${1:?usage: setup-demo.sh <sandbox-home>}"
rm -rf "$DEMO_HOME"
mkdir -p "$DEMO_HOME"

# Package caches — sizes chosen to look like a real dev machine.
mk() { mkdir -p "$1"; mkfile -n "$2" "$1/blob.bin" 2>/dev/null || head -c "$2" /dev/zero > "$1/blob.bin"; }

mk "$DEMO_HOME/.cache/uv/archive-v0/pkg"        1g
mk "$DEMO_HOME/.cache/pip/wheels"               420m
mk "$DEMO_HOME/.npm/_cacache/content-v2"        380m
mk "$DEMO_HOME/.cargo/registry/cache/crates"    260m
mk "$DEMO_HOME/.cache/huggingface/hub/models"   700m

# Project build artifacts under a projects root.
mk "$DEMO_HOME/projects/webapp/node_modules/dep"       880m
mk "$DEMO_HOME/projects/webapp/.next/cache"            310m
mk "$DEMO_HOME/projects/api/.venv/lib"                 540m
mk "$DEMO_HOME/projects/engine/target/debug"           620m

echo "seeded $DEMO_HOME"
