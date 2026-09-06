# What cachewipe cleans

Every target is declared in `src/targets.rs`. This is the complete, auditable
list — cachewipe cannot delete anything not derived from an entry here.

Where a target lists several paths, all of them are checked and every one that
exists is reported. That's deliberate: the same tool stores its cache in
different places per platform, and checking only one location meant reporting
"not present" while gigabytes sat in the other.

## Package caches — on by default (fully regenerable)

| id | path(s) | regenerates |
|---|---|---|
| uv | `~/.cache/uv`, `~/Library/Caches/uv` | next `uv sync` / `uv pip install` |
| pip | `~/.cache/pip`, `~/Library/Caches/pip` | next `pip install` |
| npm | `~/.npm/_cacache` | next `npm install` |
| npx | `~/.npm/_npx` | next `npx <pkg>` run |
| yarn | `~/.cache/yarn`, `~/Library/Caches/Yarn` | next `yarn install` |
| pnpm | `~/.local/share/pnpm/store`, `~/Library/pnpm/store` | next pnpm install |
| cargo-registry | `~/.cargo/registry/cache` | next `cargo build` |
| go-mod | `~/go/pkg/mod/cache/download` | next `go build` |
| gradle | `~/.gradle/caches` | next Gradle build |
| huggingface | `~/.cache/huggingface` | re-downloaded from the hub |

`npx` is separate from `npm` because `_npx` holds fully-installed throwaway
package trees rather than tarballs, and routinely outgrows `_cacache` several
times over.

Only the pnpm **store** is ever a target. `PNPM_HOME` (`~/Library/pnpm`) also
holds the pnpm binary and your global installs, so the parent directory is
off-limits.

## Versioned tool installs — on by default, newest kept

| id | path(s) | keeps |
|---|---|---|
| puppeteer | `~/.cache/puppeteer/{chrome,chrome-headless-shell,firefox}` | newest 1 per product |
| playwright | `~/Library/Caches/ms-playwright`, `~/.cache/ms-playwright` | newest 1 per product |

These accumulate one entire browser per release — 8 Chrome builds and 4 Chromiums
is a normal state for a machine that runs browser tests. All-or-nothing is the
wrong operation here: deleting the directory forces a re-download of the version
in use, so cachewipe reclaims only the stale ones.

Children are grouped by the text before their last `-`, so a mixed directory
stays correct: `chromium-1187` and `firefox-1490` are different products and each
keeps its own newest. A child with no `-` is its own group and is therefore always
kept — an unfamiliar layout does nothing rather than something wrong. Ordering is
by directory mtime, which is install time in practice and avoids parsing the many
version-string dialects these tools use.

## Build artifacts — on by default, but only under an explicit `--root`

| id | dir name | requires beside it | regenerates |
|---|---|---|---|
| node_modules | `node_modules` | — | `npm install` / `pnpm install` |
| venv | `.venv` | — | `uv sync` / `python -m venv` |
| next | `.next` | — | `next build` |
| cargo-target | `target` | `Cargo.toml`, `pom.xml`, `build.gradle`, `build.gradle.kts` | `cargo build` / `mvn package` |
| pycache | `__pycache__` | — | recompiled on next import |

Scanned only when you pass `--root PATH`, so cachewipe never walks your whole
home directory unprompted.

`target` is the one name that needs proof. It's Cargo's build directory, but
`source/` + `target/` is also an everyday data-directory convention in ETL and ML
repos, and nothing regenerates that. So `target` only matches when a build
manifest sits next to it. When the manifest is absent cachewipe keeps walking
*into* the directory rather than pruning, so a real project nested inside a data
tree is still found.

Nested candidates are deduped across targets before anything is sized. Targets
resolve independently, so `.next/standalone/…/node_modules` matches the
node_modules target while `.next` matches its own, and `__pycache__` dirs turn up
inside an already-matched `.venv`. Counting both double-reports the same bytes and
inflates the total; the outer path wins, since deleting it removes the inner one
anyway.

## Delegated cleanups — on by default, run the tool's own command

| id | command | scope |
|---|---|---|
| docker | `docker system prune -f` | dangling images + build cache only |
| homebrew | `brew cleanup --prune=all` | stale downloads and old installed versions |

Never `docker system prune -a` (would remove tagged images) and never
`--volumes` (would remove named volumes / data). cachewipe never manipulates
these tools' files directly.

**Docker's prune does not shrink the VM disk file.** It frees space *inside* the
VM; on macOS and Windows the host may never get those blocks back. If the goal is
host disk space, the `docker-desktop-disk` target below is what delivers it.
These entries are reported without a size because the engine owns those numbers.

**These are the only targets that are not confined by `$HOME`.** Every path
target resolves under `$HOME`, so redirecting it sandboxes the run — but a
delegated cleanup is a subprocess that reads its own config and reaches the real
machine whatever `$HOME` says. Pass `--no-external` when running against a
fixture or scratch home; an `--apply` run with a redirected `$HOME` also warns.

## Container VM disks — always reported, OFF by default to delete

| id | path | on delete |
|---|---|---|
| docker-desktop-disk | `~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw` | Docker Desktop recreates an empty disk on next launch |
| finch-disk | `~/.finch/.disks` | `finch vm init` recreates it |
| lima-disk | `~/.lima` | `limactl start` recreates the machine |
| colima-disk | `~/.colima` | `colima start` recreates the machine |
| podman-disk | `~/.local/share/containers/podman` | `podman machine init` recreates it |

This tier exists because a container VM disk is usually the single largest
reclaimable object on a developer machine — tens of gigabytes is normal — and a
cleanup tool that stays silent about it is not doing its job. So these are always
sized and shown, and counted under `total_opt_in_bytes` rather than the headline
total.

Deleting one destroys **every local image, container, and named volume** for that
engine, which is a much heavier loss than any package cache. Hence
`--include-vm-disks`. Two things worth checking first:

- **Stop the engine.** Removing a live VM's disk risks a corrupt machine.
- **Look for an orphan.** A disk whose VM no longer exists (`finch vm status` →
  `Nonexistent`) is pure waste and the easiest large win on the machine.

Note that `Docker.raw` is a sparse *file*, not a directory, and its apparent
length is far larger than its allocated size. cachewipe reports allocated blocks,
which is what the filesystem actually returns.

## OS / app caches — OFF by default (opt-in)

| id | path | regenerates |
|---|---|---|
| spotify-cache | `~/Library/Caches/com.spotify.client` | re-streamed on next play |
| firefox-cache | `~/Library/Caches/Firefox` | refetched as you browse |
| chrome-cache | `~/Library/Caches/Google` | refetched as you browse |
| jsii-cache | `~/Library/Caches/com.amazonaws.jsii` | rebuilt on next CDK synth |

Enable with `--include-os-caches` only.

These are named individually rather than as one blanket `~/Library/Caches` entry.
The blanket version could only ever be all-or-nothing over a directory where some
apps keep semi-durable state, which made the opt-in unreasonable to accept — so
nobody accepted it and the whole tier went unused. Naming the large, plainly
regenerable ones turns it into a decision someone can actually make. The cost is
coverage: an app cache with no entry here is simply not touched.

## Site-specific and corporate-internal caches

Deliberately absent. On a work machine these are often the *biggest* win — one
real example had 53 stacked versions of a single internal tool manager, and a
shared build-system package cache in the tens of gigabytes — but internal tool
names and paths don't belong in a public catalog, and the right cleanup command
is usually the vendor's own rather than an `rm`.

To carry them locally, add entries to `catalog()` in `src/targets.rs` on a branch
you don't push:

- If the tool has its own cleanup subcommand, prefer `Kind::External` and let it
  decide what's safe. Many keep a rollback version on purpose.
- If it's a directory of stacked versions, `Kind::VersionedDirs` with `keep: 1`
  is usually right.
- If a live workspace can reference the cache, remember that lockfile-based in-use
  detection won't see that. Prefer reporting it over deleting it by default.
