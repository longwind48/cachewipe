<div align="center"><pre>
▄▀▀ ▄▀▄ ▄▀▀ █ █ █▀▀ █ █ █ █▀▄ █▀▀
█   █▀█ █   █▀█ █▀▀ █▄█ █ █▀  █▀▀
▀▄▄ ▀ ▀ ▀▄▄ ▀ ▀ ▀▀▀ ▀▀▀ ▀ ▀   ▀▀▀

Reclaim your disk. Delete nothing you'll miss.
</pre></div>

<p align="center"><strong>Rust CLI · agent skill · dry-run by default · allowlist-only deletion · lock-aware · safe in a loop</strong></p>

<p align="center">
  <a href="https://github.com/longwind48/cachewipe/actions/workflows/ci.yml"><img src="https://github.com/longwind48/cachewipe/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <a href="#safety"><img src="https://img.shields.io/badge/deletes-only%20on%20--apply-brightgreen.svg" alt="Dry-run by default"></a>
  <a href="#tests"><img src="https://img.shields.io/badge/tests-15%20passing-brightgreen.svg" alt="15 tests"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust stable">
  <img src="https://img.shields.io/badge/macOS%20%7C%20Linux%20%7C%20WSL-supported-lightgrey.svg" alt="Platforms: macOS, Linux, WSL">
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#install">Install</a> ·
  <a href="#what-it-cleans">What it cleans</a> ·
  <a href="#safety">Safety</a> ·
  <a href="#benchmarks">Benchmarks</a> ·
  <a href="#run-it-weekly">Run it weekly</a> ·
  <a href="SECURITY.md">Security</a>
</p>

---

## The problem

**"Your disk is almost full."** `ENOSPC` mid-build. An install that dies at 97%.

So you go hunting — delete some downloads, empty the trash, buy back 2 GB, full
again next week. The real culprit is invisible: **package caches and build
artifacts.**

| | Typical size | Source |
|---|---|---|
| One `node_modules` | **340 MB** median (720 MB at p75) | [measured across npm projects, 2026](https://enterno.io/en/s/research-npm-dependencies-median-2026) |
| 10–20 projects' worth | 5–15 GB | [reported range](https://www.cluttered.dev/blog/delete-node-modules) |
| Docker, left unpruned | tens of GB | [reported range](https://khides.com/en/blog/developer-disk-cleanup/) |
| A neglected package cache | **340 GB** | this tool exists because of one |

That last row is real, not hypothetical — package managers rarely evict anything,
so a cache pinned to `@latest` keeps every version it ever downloaded.

**None of it is data.** Every byte regenerates: `npm install` re-downloads it,
`cargo build` remakes it. It's the safest space on your disk to delete and usually
the largest — people leave it because `rm -rf` with a glob at 2am is a bad idea
and telling cache from work is genuinely hard.

cachewipe draws that line for you, shows the number first, and deletes nothing
until you say so.

<p align="center">
  <img src="demo/demo.gif" alt="cachewipe in action" width="900">
  <br/><sub>Dry-run reports 5.0 GB across 9 caches — then <code>--apply</code> reclaims it.<br/>
  Recorded against a sandbox home; re-render with <code>vhs demo/demo.tape</code>.</sub>
</p>

### It's a harness, not a prompt

Ask an agent to free up disk space and it will improvise `rm -rf` from a bash
tool. That works right up until it doesn't: bash hands the harness an opaque
command string, so nothing can inspect what's about to be deleted or stop it.

cachewipe replaces that with a **dedicated, gated tool**. Deletion becomes a typed
action the harness can intercept and audit instead of a shell string it has to
trust:

- **Bounded** — deletes only paths resolved from an explicit catalog
  ([`src/targets.rs`](src/targets.rs)). No arbitrary-path delete exists in the code,
  so no prompt can talk it into one.
- **Refuses on doubt** — `/`, `$HOME`, symlink escapes and caches held by a live
  lockfile are declined. [`src/safety.rs`](src/safety.rs) is 184 readable lines;
  [15 tests](#tests) prove it against a real filesystem.
- **Dry-run by default** — reports first, deletes only on `--apply`, so a
  scheduled run can't surprise you.
- **Scanning chosen by measurement, not vibes** — 200k files in 609 ms, faster
  than every Rust/Go disk-usage tool it was [benchmarked](#benchmarks) against.
- **Offline** — no network code, no telemetry, two dependencies (`serde`,
  `serde_json`). [SECURITY.md](SECURITY.md) has the threat model.

## Quick start

**1. Install it** into whichever coding assistants you use:

```bash
npx skills add longwind48/cachewipe
```

**2. Call it.** No flags to learn:

```
/cachewipe
```

It reports what it found, waits for your OK, then reclaims it. You can add
context in the same breath — `/cachewipe ~/projects` or `/cachewipe just tell me
what's reclaimable` — or skip the slash entirely and say "I'm low on disk space",
which triggers it too.

**3. Make it automatic.** Wrap that call in a weekly loop and stop thinking about
disk space:

```
/loop 7d /cachewipe
```

That's Claude Code's `/loop`; other assistants have their own scheduling verb.
The skill handles the rest — it dry-runs first and age-gates so the project
you're actively building never disappears from under you.

<details>
<summary><b>Prefer the raw CLI?</b> It's a normal binary — no assistant needed.</summary>

```bash
cachewipe --root ~/projects            # report; deletes nothing
cachewipe --apply --root ~/projects    # reclaim it
```

See [All the flags](#all-the-flags) and [Install](#install) for building from
source.
</details>

## Install

`npx skills add longwind48/cachewipe` detects whichever coding assistants you
have and asks where to install. It isn't tied to one vendor —
[`npx skills`](https://github.com/vercel-labs/skills) supports Claude Code,
Codex, Cursor, Zed, Warp, Cline, Continue, Crush, OpenClaw, Amp, Replit and dozens
more. To skip the prompt:

```bash
npx skills add longwind48/cachewipe --agent '*' -y      # every agent it finds
npx skills add longwind48/cachewipe -a codex -a cursor  # or name them
```

**Just want the binary, no assistant?** It's a plain CLI:

```bash
git clone https://github.com/longwind48/cachewipe && cd cachewipe
cargo build --release
./target/release/cachewipe --help
```

Put `target/release/cachewipe` on your `PATH` to use the short commands above.
No Rust toolchain? There's a pure-shell fallback with the same targets in
[`references/fallback.md`](references/fallback.md).

### Platform support

| Environment | Works? |
|---|---|
| macOS (Terminal, iTerm, Ghostty…) | ✅ Yes |
| Linux | ✅ Yes |
| **Windows via WSL2** | ✅ Yes — install and run inside the WSL shell |
| Windows: PowerShell / cmd.exe natively | ❌ **No** |

**Windows users need WSL.** Being straight about why, rather than implying
partial support: cachewipe resolves your home directory from `$HOME`, which
Windows doesn't set (it uses `%USERPROFILE%`), so it exits immediately. The cache
catalog also only contains Unix paths — the Windows equivalents live under
`%LOCALAPPDATA%` and aren't in it. And CI only builds and tests on Linux and
macOS, so Windows is genuinely unverified, not just undocumented.

Inside WSL it's a normal Linux install and works fully — but note it cleans the
caches of your *Linux* home, not `C:\Users\you\AppData`. Native Windows support
is a welcome contribution — it needs a `USERPROFILE` fallback in `src/main.rs`,
`%LOCALAPPDATA%` entries in `src/targets.rs`, and `windows-latest` added to the
CI matrix.

## All the flags

```bash
cachewipe                        # package caches + docker only (no --root)
cachewipe --root <dir>           # also scan <dir> for build artifacts; repeatable
cachewipe --apply                # delete instead of report
cachewipe --min-age-days 14      # skip anything used in the last 14 days
cachewipe --include-os-caches    # opt in to ~/Library/Caches (off by default)
cachewipe --json                 # machine-readable output
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

Every rule below is enforced in [`src/safety.rs`](src/safety.rs) and asserted by
[the tests](#tests) — not left as a README promise.

| Guarantee | What it means |
|---|---|
| Allowlist only | Deletes only paths resolved from `src/targets.rs`. No arbitrary-path delete exists. |
| Protected paths | `/`, `$HOME`, any ancestor of home, anything under two components deep — all hard-refused. |
| No symlink escape | Paths are canonicalized and confined to their root; symlinks are never traversed. |
| Lock-aware | A fresh package-manager lockfile means "in use" — declined rather than corrupted. |
| Docker delegated | `docker system prune -f` only. Never `-a` (tagged images), never `--volumes` (your data). |
| Dry-run default | `--apply` is the only way anything is removed. |

## Run it weekly

Scheduling is where this earns its keep: you stop discovering the problem at the
moment a build dies. [Quick start step 3](#quick-start) shows the one-liner —
this is the detail behind it.

**Why it's safe to automate.** Two defaults do the work. Dry-run means a
scheduled run reports unless you explicitly asked it to delete, and
`--min-age-days` skips anything you've touched recently, so the `node_modules` of
whatever you're actively building never disappears from under you. Ask for a
weekly *report* rather than a cleanup and you'll get that instead — the skill
follows the intent you state.

**Prefer a plain cron job?** It's an ordinary CLI, so schedule the command
directly — Mondays at 9am, age-gated to two weeks:

```bash
(crontab -l 2>/dev/null; echo "0 9 * * 1 $HOME/.cargo/bin/cachewipe --apply --min-age-days 14 --root $HOME/projects") | crontab -
```

Drop `--apply` if you'd rather be told the number and decide for yourself.

## Tests

```bash
cargo test    # 15 tests: 10 unit guardrails + 5 integration
```

The integration suite builds a real temporary home and asserts on the
filesystem afterward — that dry-run leaves every byte in place, that `--apply`
removes exactly what it reported, that a locked cache survives, that OS caches
stay off without the flag, and that artifacts need a `--root`.

## Benchmarks

Sizing 200k files (~800 MB), M4 Mac / APFS, via `bash bench/bench.sh`
([hyperfine](https://github.com/sharkdp/hyperfine), 10 runs):

| `du -sk` | **cachewipe** | `dust` | `diskus` |
|---|---|---|---|
| 408 ms | **609 ms** | 3,961 ms | 5,008 ms |

Faster than the Rust/Go disk-usage tools, within 1.5× of C `du`. A parallel walk
was tried and measured 4.7× *slower* — see [`src/scan.rs`](src/scan.rs) for why,
before you optimise it back.

## Trust note

`npx skills add` fetches and runs code from this repo. Before installing
anything that can delete files, skim the source — it's deliberately small
(`src/targets.rs` for what it touches, `src/safety.rs` for how it refuses
everything else) — or pin to a tagged commit instead of `main`. The binary is
compiled locally from that source; nothing prebuilt is ever downloaded.

## License

MIT — see [LICENSE](LICENSE).
