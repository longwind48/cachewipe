# Pure-shell fallback (no Rust)

Same targets, same dry-run-first discipline. Slower, and without the lockfile
in-use detection the Rust binary has — so be more conservative about `--apply`.

## Report (dry-run — deletes nothing)

```bash
# Package caches — size only
for d in ~/.cache/uv ~/.cache/pip ~/.npm/_cacache ~/.cache/yarn \
         ~/.cargo/registry/cache ~/.gradle/caches ~/.cache/huggingface; do
  [ -d "$d" ] && du -sh "$d" 2>/dev/null
done

# Docker (delegated to the engine)
command -v docker >/dev/null && docker system df

# Build artifacts under a projects dir (report)
find ~/projects -type d \( -name node_modules -o -name .venv -o -name .next \
  -o -name target -o -name __pycache__ \) -prune -print 2>/dev/null \
  | while read -r p; do du -sh "$p" 2>/dev/null; done
```

## Apply (only after reviewing the report)

```bash
# Package caches — prefer the tool's own clean where it exists (respects locks)
uv cache clean 2>/dev/null || rm -rf ~/.cache/uv
pip cache purge 2>/dev/null
npm cache clean --force 2>/dev/null

# Docker: dangling only — never -a or --volumes without explicit intent
docker system prune -f

# Build artifacts (BE CAREFUL — confirm the root first)
find ~/projects -type d \( -name node_modules -o -name .venv -o -name .next \
  -o -name target -o -name __pycache__ \) -prune -exec rm -rf {} +
```

## The lock lesson (why the Rust version is safer)

Package managers keep a `.lock` file while operating on their cache. Deleting a
cache mid-operation corrupts the running install. The shell version can't cheaply
detect this — prefer each tool's own `clean` subcommand (`uv cache clean`,
`npm cache clean`), which waits on the lock, over a raw `rm -rf` of the cache dir.
If a `clean` reports the cache is in use, stop and let the process finish rather
than forcing it.
