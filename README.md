<div align="center"><pre>
▄▀▀ ▄▀▄ ▄▀▀ █ █ █▀▀ █ █ █ █▀▄ █▀▀
█   █▀█ █   █▀█ █▀▀ █▄█ █ █▀  █▀▀
▀▄▄ ▀ ▀ ▀▄▄ ▀ ▀ ▀▀▀ ▀▀▀ ▀ ▀   ▀▀▀

Reclaim your disk. Delete nothing you'll miss.
</pre></div>

<p align="center"><strong>Rust CLI · Claude skill · dry-run by default · allowlist-only deletion · lock-aware · safe in a loop</strong></p>

<p align="center">
  <a href="https://github.com/longwind48/cachewipe/actions/workflows/ci.yml"><img src="https://github.com/longwind48/cachewipe/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <a href="#safety"><img src="https://img.shields.io/badge/deletes-only%20on%20--apply-brightgreen.svg" alt="Dry-run by default"></a>
  <a href="#tests"><img src="https://img.shields.io/badge/tests-15%20passing-brightgreen.svg" alt="15 tests"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust stable">
  <img src="https://img.shields.io/badge/macOS%20%7C%20Linux-supported-lightgrey.svg" alt="Platforms">
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#what-it-cleans">What it cleans</a> ·
  <a href="#safety">Safety</a> ·
  <a href="#run-it-on-a-schedule">Scheduling</a> ·
  <a href="SECURITY.md">Security</a>
</p>

---

Caches and build artifacts are the biggest, dumbest thing on your disk: gigabytes
your package manager will happily re-download and your build will happily
regenerate. cachewipe finds them fast, tells you exactly what it would free, and
deletes **nothing** until you say so.

<p align="center">
  <img src="demo/demo.gif" alt="cachewipe in action" width="900">
  <br/><sub>Dry-run reports 5.0 GB across 9 caches — then <code>--apply</code> reclaims it.<br/>
  Recorded against a sandbox home; re-render with <code>vhs demo/demo.tape</code>.</sub>
</p>

## Quick start

**As a Claude skill** — say "I'm low on disk space" and it handles the rest:

```bash
npx skills add longwind48/cachewipe -a claude-code
```

**As a standalone CLI:**

```bash
git clone https://github.com/longwind48/cachewipe && cd cachewipe
cargo build --release
./target/release/cachewipe                    # report; deletes nothing
./target/release/cachewipe --root ~/projects  # include build artifacts
```

No Rust? There's a pure-shell fallback with the same targets in
[`references/fallback.md`](references/fallback.md).

## Usage

```bash
cachewipe                                   # report package caches + docker
cachewipe --root ~/projects                 # also scan projects for build artifacts
cachewipe --apply --root ~/projects         # actually delete
cachewipe --apply --min-age-days 14 ...     # unattended-safe: skip recently used
cachewipe --include-os-caches               # opt in to ~/Library/Caches
cachewipe --json                            # machine-readable
```

## What it cleans

Everything here regenerates. The full catalog lives in
[`src/targets.rs`](src/targets.rs) — that file is the only way cachewipe learns
about a deletable thing, so it's short and auditable on purpose.

| Tier | Targets | Default |
|---|---|---|
| **Package caches** | uv · pip · npm · yarn · pnpm · cargo · go · gradle · huggingface | ✅ on |
| **Build artifacts** | `node_modules` · `.venv` · `.next` · `target` · `__pycache__` | ✅ on, needs `--root` |
| **Docker** | dangling images + build cache (via `docker system prune -f`) | ✅ on |
| **OS / app caches** | `~/Library/Caches` | ⛔ opt-in |

Build artifacts are only scanned under a `--root` you name, so cachewipe never
walks your home directory uninvited. OS caches are off by default because
"regenerable" isn't guaranteed for every app that writes there.

## Safety

The reason to trust this over a clever `rm -rf` one-liner: the rules live in
tested code, not in a README promise.

- **Allowlist only** — deletion operates solely on paths resolved from
  `src/targets.rs`. There is no arbitrary-path delete anywhere in the codebase.
- **Protected paths refused** — `/`, `$HOME`, any ancestor of home, and anything
  shallower than two path components are hard-refused.
- **No symlink escape** — paths are canonicalized and must stay inside their
  root; symlinks are never traversed while sizing or deleting.
- **Lock-aware** — a fresh package-manager lockfile marks a cache as in use and
  cachewipe declines it. Deleting a cache mid-install corrupts it; this check
  exists because that actually happened.
- **Docker stays delegated** — `docker system prune -f` only. Never `-a` (would
  drop tagged images), never `--volumes` (would drop your data).
- **Dry-run default** — `--apply` is the only way to remove anything.

Read [`src/safety.rs`](src/safety.rs) (184 lines) to check all of that yourself.

## Run it on a schedule

Dry-run-by-default is what makes recurring use safe. In Claude Code:

```
/loop 1d cachewipe --root ~/projects
```

That reports drift daily and touches nothing. For hands-off cleanup, age-gate it
so active projects are left alone:

```bash
cachewipe --apply --min-age-days 14 --root ~/projects
```

## Tests

```bash
cargo test    # 15 tests: 10 unit guardrails + 5 integration
```

The integration suite builds a real temporary home and asserts on the
filesystem afterward — that dry-run leaves every byte in place, that `--apply`
removes exactly what it reported, that a locked cache survives, that OS caches
stay off without the flag, and that artifacts need a `--root`.

## Trust note

`npx skills add` fetches and runs code from this repo. Before installing
anything that can delete files, skim the source — it's deliberately small
(`src/targets.rs` for what it touches, `src/safety.rs` for how it refuses
everything else) — or pin to a tagged commit instead of `main`. The binary is
compiled locally from that source; nothing prebuilt is ever downloaded.

## License

MIT — see [LICENSE](LICENSE).
