---
name: cachewipe
description: >-
  Reclaim disk space by removing regenerable cache and build files — uv/pip/npm/cargo/go
  package caches, node_modules/.venv/target/.next build artifacts, and dangling Docker
  images. Reports what it would free by default and deletes nothing until you say so, so
  it is safe to run on a schedule. Use this skill whenever the user is low on disk space,
  mentions a full or nearly-full disk, asks to "free up space", "clean caches", "clear
  build artifacts", "reclaim storage", says their machine or Docker is bloated, or wants a
  recurring cleanup job (a /loop). Also use it before large builds, downloads, or installs
  that might fail for lack of space, even if the user does not name a specific cache.
---

# cachewipe

A fast Rust tool that finds and removes **regenerable** files — caches and build
artifacts that a package manager or build command will simply recreate. It never
touches source, config, or data. It **reports by default and deletes only with
`--apply`**, which is what makes it safe to run unattended in a `/loop`.

## The one thing to remember

Dry-run first, always. A bare run tells the user what's reclaimable and deletes
nothing. Only add `--apply` after they've seen the plan (or explicitly asked to
just clean it).

## Invoked with nothing to go on

`/cachewipe` with no arguments is the common case, so handle it without a round
of questions. Package caches need no configuration — scan them immediately. Build
artifacts need a `--root`, so infer one rather than asking: if the current
directory is inside a git repo or a projects tree, use that; otherwise check for
a conventional `~/projects`, `~/code`, `~/dev`, or `~/src` and name the one you
picked in your summary so the user can correct it. If none exists, report package
caches alone and mention that passing a directory would also cover build
artifacts. Only ask when the user says something ambiguous like "clean
everything" — there, confirming beats guessing, because `--include-os-caches` is
the one flag that can remove something a user might miss.

## Step 1: Ensure the binary exists

The tool is a small Rust binary. Build it once; reuse forever. From the skill
directory:

```bash
BIN="$(dirname "$0")/target/release/cachewipe"   # if invoked with a path; else use the skill dir
# Prefer an already-built binary:
if [ ! -x "$BIN" ]; then
  if command -v cargo >/dev/null 2>&1; then
    cargo build --release --manifest-path "<skill-dir>/Cargo.toml" >/tmp/cachewipe-build.log 2>&1 \
      && echo "built cachewipe" || { echo "build failed — see /tmp/cachewipe-build.log"; }
  else
    echo "cargo not found."
  fi
fi
```

If `cargo` is missing, tell the user: install Rust with
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, or fall back to
the shell equivalent in `references/fallback.md` (same targets, same dry-run-first
discipline, just slower and without the lock-safety niceties).

## Step 2: Report (dry-run)

```bash
cachewipe --json                 # package caches + docker, safe defaults
cachewipe --json --root ~/projects   # also scan a projects dir for build artifacts
```

The JSON gives one object per target with `bytes`, `files`, `regenerates`,
`verdict` (`reclaimable`/`skipped`), and `reason`. Summarize for the user: total
reclaimable, the biggest items, and anything skipped (and why — e.g. "in use").

Present it plainly. Lead with the number that matters: "~X GB reclaimable." Then
the top few targets. Don't bury it.

## Step 3: Apply (only when the user is on board)

```bash
cachewipe --apply --root ~/projects
```

Docker is delegated to `docker system prune -f` (dangling images + build cache
only — never `-a`, never `--volumes`, so tagged images and named volumes are
safe). Everything else is a direct, bounded `remove_dir_all` of a path the tool
already proved is inside an allowed cache/build location.

## Running in a /loop

This is the intended recurring-cleanup mode. Because dry-run is the default, a
loop that runs `cachewipe --json` reports drift over time and touches nothing —
the user reviews and decides when to `--apply`. For hands-off cleanup, the safe
recurring form is:

```bash
cachewipe --apply --min-age-days 14 --root ~/projects
```

`--min-age-days` only deletes cache/artifact dirs whose newest file is older than
N days, so an actively-used project's `node_modules` is left alone. Recommend 14+
for unattended loops.

## Guardrails (why this is trustable)

These live in tested Rust code (`src/safety.rs`, `tests/integration.rs`), not in
prose you have to trust me to follow:

- **Allowlist only.** The tool can only ever delete paths that a target in
  `src/targets.rs` resolved. There is no arbitrary-path delete.
- **Protected paths refused.** `$HOME`, `/`, and any ancestor of home are hard-
  refused even if a target somehow resolved to them.
- **No symlink escape.** Paths are canonicalized and must stay within their root;
  symlinks are never traversed during sizing or deletion.
- **In-use detection.** A fresh lockfile (uv/npm/cargo `.lock`) marks a cache as
  active; the tool refuses to delete it rather than corrupt a running install.
  (This is a real lesson — deleting a locked uv cache mid-use breaks it.)
- **OS/app caches are opt-in** (`--include-os-caches`), off by default, because a
  few apps keep semi-durable state under `~/Library/Caches`.

## Reference

- `references/fallback.md` — pure-shell version for machines without Rust.
- `references/targets.md` — the full list of what's cleaned and how it regenerates.
