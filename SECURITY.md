# Security

cachewipe deletes files. It is built to be audited before you trust it.

## Threat model

The risk is deleting something the user did not intend to lose. cachewipe
mitigates this structurally:

1. **No arbitrary deletion.** The only paths ever passed to `remove_dir_all`
   come from resolving a `Target` in `src/targets.rs`. There is no code path
   that deletes a user-supplied or discovered arbitrary path.
2. **Bounded discovery.** Home-dir caches are exact subpaths. Build-artifact
   scanning happens only under a `--root` the user names, and matches only fixed
   directory names, pruning at first match.
3. **Protected-path refusal.** `src/safety.rs` hard-refuses `/`, `$HOME`, any
   ancestor of home, and any path shallower than two components.
4. **Symlink confinement.** Paths are canonicalized (resolving symlinks in the
   existing prefix) and must remain within their declared root. Sizing and
   deletion never traverse symlinks.
5. **Dry-run default.** Nothing is deleted without `--apply`.
6. **No network, no shell injection surface.** The only subprocess is `docker`
   with fixed arguments. No user input is interpolated into a shell.

## Reviewing before you trust it

Two short files tell you everything it can do:

- `src/targets.rs` — the complete catalog of what may be deleted.
- `src/safety.rs` — the checks that refuse everything else (unit-tested).

`tests/integration.rs` proves the end-to-end behavior against a real temp
filesystem: dry-run deletes nothing, `--apply` deletes only the claimed target,
locked caches survive, OS caches stay off by default.

## Reporting a vulnerability

Open a private security advisory on the repository, or email the maintainers.
Please do not open a public issue for a vulnerability that could cause data loss.
