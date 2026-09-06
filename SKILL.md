---
name: cachewipe
description: >-
  Reclaim disk space by removing regenerable cache and build files — uv/pip/npm/pnpm/cargo/go
  package caches, node_modules/.venv/target/.next build artifacts, stale browser-driver
  downloads, and dangling Docker images. Also sizes and reports container VM disk images
  (Docker Desktop, Finch, Lima, Colima, Podman), which are usually the largest reclaimable
  thing on a developer machine. Reports what it would free by default and deletes nothing
  until you say so, so it is safe to run on a schedule. Use this skill whenever the user is
  low on disk space, mentions a full or nearly-full disk, asks to "free up space", "clean
  caches", "clear build artifacts", "reclaim storage", "what's eating my disk", says their
  machine or Docker is bloated, or wants a recurring cleanup job (a /loop). Also use it
  before large builds, downloads, or installs that might fail for lack of space, even if the
  user does not name a specific cache.
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
everything" — there, confirming beats guessing, because the opt-in flags are the
ones that can remove something a user would miss.

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
cachewipe                            # package caches + delegated cleanups
cachewipe --root ~/projects          # also scan a projects dir for build artifacts
```

**Read the default (human) output — don't reach for `--json` to summarize.** The
tool already does the rollup: totals per target, the largest individual paths, a
"held back (needs a flag)" section, and a one-line count of targets absent from
this machine. Piping JSON into a throwaway script to re-derive that is wasted
work, and it's easy to mis-key the shape. Use `--json` only when you genuinely
need to compute something the report doesn't show, and `--top N` when you want
more or fewer individual paths than the default 12.

The JSON shape, when you do need it:

```json
{
  "dry_run": true,
  "total_reclaimable_bytes": 0,
  "total_opt_in_bytes": 0,
  "total_deleted_bytes": 0,
  "free_bytes_before": 0,
  "free_bytes_after": null,
  "items": [
    { "id": "uv", "tier": "package-cache", "path": "…", "bytes": 0,
      "files": 0, "regenerates": "…", "verdict": "reclaimable",
      "reason": "dry-run" }
  ]
}
```

Note the top-level key is `items` (not `targets`), and there are **two** totals
that matter:

- `total_reclaimable_bytes` — what `--apply` would free **with the current
  flags**. This is the number to lead with, because it's the only one you can
  promise.
- `total_opt_in_bytes` — sized and shown, but gated behind an opt-in flag. On a
  machine with a container engine this is often far larger than the first number.
  Surface it as a separate offer, never folded into the headline.

Present it plainly. Lead with the number that matters: "~X GB reclaimable." Then
the top few targets, then the opt-in offer if there is one. Don't bury it.

### Free space is reported from the right volume

`free_bytes_before` comes from the data volume, not `/`. This matters on macOS:
`df /` reports the sealed read-only system volume, which is always near-full and
has nothing to do with reclaimable space. If you quote free space from your own
`df` call, use `/System/Volumes/Data` on macOS or you will confidently tell the
user the wrong thing.

## Step 3: Apply (only when the user is on board)

```bash
cachewipe --apply --root ~/projects
```

Two tiers are deliberately excluded from that and need to be asked for:

- `--include-os-caches` — named app caches under `~/Library/Caches` (Spotify,
  browser caches, CDK's jsii cache). Individually named rather than the whole
  directory, so opting in is a decision someone can actually reason about.
- `--include-vm-disks` — container VM disk images. These are **always sized and
  reported** because they're usually the biggest reclaimable object on the disk,
  but deleting one destroys every local image, container, and named volume for
  that engine. Say that plainly before you use this flag. If the engine is
  running, stop it first: removing a live VM's disk risks a corrupt machine.

Delegated cleanups (`docker system prune -f`, `brew cleanup --prune=all`) run the
tool's own command rather than removing files. **Docker's prune does not shrink
the VM disk file** — it frees space *inside* the VM, which the host may never get
back. If the user's goal is host disk space, the `docker-desktop-disk` item is
the one that delivers it, not the prune. Everything else is a bounded
`remove_dir_all`/`remove_file` of a path the tool already proved is inside an
allowed location.

## Running in a /loop

This is the intended recurring-cleanup mode. Because dry-run is the default, a
loop that runs `cachewipe` reports drift over time and touches nothing — the user
reviews and decides when to `--apply`. For hands-off cleanup, the safe recurring
form is:

```bash
cachewipe --apply --min-age-days 14 --root ~/projects
```

`--min-age-days` only deletes cache/artifact dirs whose newest file is older than
N days, so an actively-used project's `node_modules` is left alone. Recommend 14+
for unattended loops. Leave the opt-in flags out of a loop — a scheduled job is
the wrong place to destroy container images.

## What the tool cannot see

Worth saying out loud to the user when it applies, because these are the cases
where a confident report is wrong:

- **Caches a live workspace depends on.** In-use detection is lockfile-based, so
  it catches a running `uv`/`npm`/`cargo` install. It cannot know that a monorepo
  or corporate build system (Bazel, Brazil, Nix) has a workspace pointing into a
  shared package cache. Deleting that is recoverable but can mean a very large
  re-download. If the machine has such a workspace, check before clearing its
  cache.
- **Anything not in the catalog.** `src/targets.rs` is the complete list. A big
  directory the tool doesn't mention isn't endorsed as safe — it's just unknown.
  When a user is genuinely out of space and the report looks small relative to
  the problem, say so and offer to look around (`du -sh ~/* ~/.[a-z]*`) rather
  than implying the report is exhaustive.
- **Site-specific and corporate-internal caches** are deliberately absent from
  the public catalog, and on a work machine they can be the largest win of all.
  See `references/targets.md` for how to carry those locally.

## Guardrails (why this is trustable)

These live in tested Rust code (`src/safety.rs`, `src/targets.rs`,
`tests/integration.rs`), not in prose you have to trust me to follow:

- **Allowlist only.** The tool can only ever delete paths that a target in
  `src/targets.rs` resolved. There is no arbitrary-path delete.
- **Protected paths refused.** `$HOME`, `/`, and any ancestor of home are hard-
  refused even if a target somehow resolved to them.
- **No symlink escape.** Paths are canonicalized and must stay within their root;
  symlinks are never traversed during sizing or deletion.
- **In-use detection.** A fresh lockfile (uv/npm/cargo `.lock`) marks a cache as
  active; the tool refuses to delete it rather than corrupt a running install.
  (This is a real lesson — deleting a locked uv cache mid-use breaks it.)
- **Ambiguous names need proof.** `target/` is Cargo's build dir *and* an ordinary
  data directory name, so it only matches with a `Cargo.toml`/`pom.xml`/Gradle
  file beside it. Matching on name alone would delete data no build can rebuild.
- **Nested candidates are deduped.** Targets resolve independently, so
  `.next/…/node_modules` and `__pycache__` inside a matched `.venv` would
  otherwise be counted twice and inflate the total. The outer path wins.
- **Versioned caches keep the newest.** Browser-driver caches accumulate one full
  browser per release; the tool reclaims the stale ones per product rather than
  wiping the version in use.
- **Both heavy tiers are opt-in** — OS/app caches (`--include-os-caches`) and VM
  disks (`--include-vm-disks`). VM disks are still *reported* so their size is
  never hidden from the user.

## Reference

- `references/fallback.md` — pure-shell version for machines without Rust.
- `references/targets.md` — the full list of what's cleaned, how it regenerates,
  and how to add site-specific targets.
