# What cachewipe cleans

Every target is declared in `src/targets.rs`. This is the complete, auditable
list — cachewipe cannot delete anything not derived from an entry here.

## Package caches — on by default (fully regenerable)

| id | path | regenerates |
|---|---|---|
| uv | `~/.cache/uv` | next `uv sync` / `uv pip install` |
| pip | `~/.cache/pip` | next `pip install` |
| npm | `~/.npm/_cacache` | next `npm install` |
| yarn | `~/.cache/yarn` | next `yarn install` |
| pnpm | `~/.local/share/pnpm/store` | next pnpm install |
| cargo-registry | `~/.cargo/registry/cache` | next `cargo build` |
| go-mod | `~/go/pkg/mod/cache/download` | next `go build` |
| gradle | `~/.gradle/caches` | next Gradle build |
| huggingface | `~/.cache/huggingface` | re-downloaded from the hub |

## Build artifacts — on by default, but only under an explicit `--root`

| id | dir name | regenerates |
|---|---|---|
| node_modules | `node_modules` | `npm install` / `pnpm install` |
| venv | `.venv` | `uv sync` / `python -m venv` |
| next | `.next` | `next build` |
| cargo-target | `target` | `cargo build` |
| pycache | `__pycache__` | recompiled on next import |

Scanned only when you pass `--root PATH`, so cachewipe never walks your whole
home directory unprompted.

## Docker — on by default, delegated to the engine

Runs `docker system prune -f`: removes **dangling images and build cache only**.
Never `-a` (would remove tagged images) and never `--volumes` (would remove named
volumes / data). cachewipe never manipulates Docker's files directly.

## OS / app caches — OFF by default (opt-in)

| id | path | note |
|---|---|---|
| os-user-cache | `~/Library/Caches` | Some apps keep semi-durable state here |

Enable with `--include-os-caches` only. Left off by default because "regenerable"
is not guaranteed for every app that writes here.
