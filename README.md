# cachewipe

**Reclaim disk space by removing regenerable cache and build files — safely.**

Package caches (uv, pip, npm, yarn, pnpm, cargo, go, gradle, huggingface),
project build artifacts (`node_modules`, `.venv`, `target`, `.next`,
`__pycache__`), and dangling Docker images. Written in Rust for speed.

The whole point: **it reports by default and deletes nothing until you pass
`--apply`.** That is what makes it safe to run on a schedule and safe to share.

```
$ cachewipe --root ~/projects
cachewipe (dry-run — nothing deleted)
----------------------------------------------------------------
   42.1GB  uv               /Users/you/.cache/uv  [dry-run]
    2.7GB  node_modules     /Users/you/projects/app/node_modules  [dry-run]
     856MB venv             /Users/you/projects/api/.venv  [dry-run]
     skip  docker           docker (dangling images + build cache)  (dry-run…)
----------------------------------------------------------------
Reclaimable: 45.6GB   (run again with --apply to delete)
```

## Install

### As a Claude Code / Claude skill (easiest)

```bash
npx skills add longwind48/cachewipe skill cachewipe -a claude-code
```

Then just tell Claude "I'm low on disk space" or set up a `/loop`. The skill
builds the binary on first run.

### As a standalone CLI

```bash
git clone https://github.com/longwind48/cachewipe && cd cachewipe
cargo build --release
./target/release/cachewipe --help
```

No Rust? A pure-shell fallback with the same targets lives in
[`references/fallback.md`](references/fallback.md).

## Usage

```bash
cachewipe                          # report package caches + docker (dry-run)
cachewipe --root ~/projects        # also report project build artifacts
cachewipe --apply --root ~/projects   # actually delete
cachewipe --apply --min-age-days 14 --root ~/projects   # unattended-safe: skip fresh dirs
cachewipe --include-os-caches      # also consider ~/Library/Caches (opt-in)
cachewipe --json                   # machine-readable output
```

## Safety guarantees

- **Allowlist only** — can delete only paths derived from `src/targets.rs`. No
  arbitrary-path deletion exists in the code.
- **Protected paths refused** — `$HOME`, `/`, and any ancestor of home are hard-
  refused.
- **No symlink escape** — paths are canonicalized and confined to their root;
  symlinks are never traversed.
- **In-use detection** — a fresh package-manager lockfile marks a cache active;
  cachewipe refuses to delete it rather than corrupt a running install.
- **Docker is delegated** — `docker system prune -f` only (dangling + build
  cache); never `-a`, never `--volumes`.
- **OS caches opt-in** — off unless `--include-os-caches`.

All of the above are covered by tests (`cargo test`).

## How it fits together

A fast Rust binary does the scanning, sizing, lock-detection, and bounded
deletion. The safety-critical logic lives in tested code, not documentation. The
Claude skill (`SKILL.md`) is a thin front door that runs the binary dry-run
first and only applies when you agree.

## License

MIT — see [LICENSE](LICENSE).

## Security

See [SECURITY.md](SECURITY.md). This tool deletes files; it is designed to be
audited. Read `src/targets.rs` (what it touches) and `src/safety.rs` (how it
refuses to touch anything else) — they are short on purpose.
