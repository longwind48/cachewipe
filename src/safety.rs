//! Guardrails. Every deletion candidate passes through here first.
//!
//! These checks exist because the alternative — trusting a path string — is how
//! cleanup tools turn into `rm -rf $HOME`. Each guard returns a `Guard` verdict
//! so the caller can report *why* something was skipped, not just that it was.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Guard {
    Ok,
    /// Path escaped its allowed root (symlink, `..`, or absolute reroute).
    OutsideRoot,
    /// A protected directory we must never delete (home root, /, etc.).
    Protected,
    /// A live process holds a lock inside this dir — deleting mid-use is unsafe.
    InUse,
    /// Path does not exist (already clean).
    Missing,
}

/// Directories that must never be deleted wholesale, regardless of catalog.
/// A cache target that resolves to one of these is a bug; we refuse it.
fn is_protected(path: &Path, home: &Path) -> bool {
    // Root, home itself, and any parent of home.
    if path == Path::new("/") || path == home {
        return true;
    }
    // Refuse anything with fewer than 2 components below root (e.g. /Users).
    let depth = path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    if depth < 2 {
        return true;
    }
    // Refuse if home is *inside* path (i.e. path is an ancestor of home).
    home.starts_with(path)
}

/// Confirm `candidate` really lives under `root` after normalizing `..` and
/// resolving symlinks on the portion that exists. We compare canonical paths so
/// a symlinked cache dir can't be used to reach outside the root.
pub fn within_root(candidate: &Path, root: &Path) -> bool {
    let cand = canonicalize_lenient(candidate);
    let base = canonicalize_lenient(root);
    cand.starts_with(&base)
}

/// canonicalize() fails if the path doesn't fully exist; walk up to the nearest
/// existing ancestor, canonicalize that, then re-append the missing tail. This
/// still resolves any symlink in the existing prefix (the security-relevant part).
fn canonicalize_lenient(p: &Path) -> PathBuf {
    let mut existing = p;
    let mut tail: Vec<Component> = Vec::new();
    loop {
        if existing.exists() {
            let mut base = existing
                .canonicalize()
                .unwrap_or_else(|_| existing.to_path_buf());
            for c in tail.iter().rev() {
                base.push(c.as_os_str());
            }
            return base;
        }
        match existing.parent() {
            Some(parent) => {
                if let Some(name) = existing.file_name() {
                    tail.push(Component::Normal(name));
                }
                existing = parent;
            }
            None => return p.to_path_buf(),
        }
    }
}

/// Full verdict for a single candidate directory.
/// `lock_present` is injected so this stays a pure function and is unit-testable
/// without a filesystem — the caller supplies the result of the lock probe.
pub fn evaluate(
    candidate: &Path,
    root: &Path,
    home: &Path,
    exists: bool,
    lock_present: bool,
) -> Guard {
    if is_protected(candidate, home) {
        return Guard::Protected;
    }
    if !within_root(candidate, root) {
        return Guard::OutsideRoot;
    }
    if !exists {
        return Guard::Missing;
    }
    if lock_present {
        return Guard::InUse;
    }
    Guard::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/alice")
    }

    #[test]
    fn refuses_home_root() {
        assert_eq!(
            evaluate(&home(), &home(), &home(), true, false),
            Guard::Protected
        );
    }

    #[test]
    fn refuses_filesystem_root() {
        assert_eq!(
            evaluate(Path::new("/"), Path::new("/"), &home(), true, false),
            Guard::Protected
        );
    }

    #[test]
    fn refuses_shallow_paths() {
        // /Users has depth 1 — an ancestor of home. Must be protected.
        assert_eq!(
            evaluate(
                Path::new("/Users"),
                Path::new("/Users"),
                &home(),
                true,
                false
            ),
            Guard::Protected
        );
    }

    #[test]
    fn refuses_ancestor_of_home() {
        // /Users/alice/.. would resolve above home.
        let p = PathBuf::from("/Users");
        assert!(is_protected(&p, &home()));
    }

    #[test]
    fn allows_legit_cache_dir() {
        let cache = home().join(".cache/uv");
        // root is the cache dir itself; exists, no lock -> Ok
        assert_eq!(evaluate(&cache, &cache, &home(), true, false), Guard::Ok);
    }

    #[test]
    fn skips_missing() {
        let cache = home().join(".cache/does-not-exist");
        assert_eq!(
            evaluate(&cache, &cache, &home(), false, false),
            Guard::Missing
        );
    }

    #[test]
    fn skips_locked() {
        let cache = home().join(".cache/uv");
        assert_eq!(evaluate(&cache, &cache, &home(), true, true), Guard::InUse);
    }

    #[test]
    fn rejects_path_outside_root() {
        let root = home().join(".cache/uv");
        let outside = home().join(".ssh");
        assert!(!within_root(&outside, &root));
    }

    #[test]
    fn accepts_path_inside_root() {
        let root = home().join(".cache");
        let inside = home().join(".cache/uv/archive-v0");
        assert!(within_root(&inside, &root));
    }
}
