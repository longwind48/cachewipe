//! Fast directory sizing and lock detection.
//!
//! Sizing walks the tree once with std::fs only. We deliberately do NOT follow
//! symlinks (both for speed and to avoid escaping the target — see safety.rs).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DirStat {
    pub bytes: u64,
    pub files: u64,
    /// Most-recent mtime seen, as seconds since epoch. Used for age-gating.
    pub newest_mtime: u64,
}

/// Recursively size a directory. Symlinks are counted as their own tiny size
/// and never traversed, so we can't be tricked into sizing /.
pub fn size_dir(path: &Path) -> DirStat {
    let mut stat = DirStat {
        bytes: 0,
        files: 0,
        newest_mtime: 0,
    };
    size_into(path, &mut stat);
    stat
}

fn size_into(path: &Path, acc: &mut DirStat) {
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        // DirEntry::metadata() does NOT follow symlinks — it's the lstat here.
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            acc.files += 1;
            continue; // never traverse symlinks
        }
        if meta.is_dir() {
            size_into(&entry.path(), acc);
        } else {
            acc.bytes += meta.len();
            acc.files += 1;
            if let Ok(m) = meta.modified() {
                if let Ok(d) = m.duration_since(UNIX_EPOCH) {
                    let secs = d.as_secs();
                    if secs > acc.newest_mtime {
                        acc.newest_mtime = secs;
                    }
                }
            }
        }
    }
}

/// Detect whether a cache dir is actively in use.
///
/// Heuristic learned the hard way: package managers (uv, npm, cargo) keep a
/// `.lock` file at the cache root while a process operates on it. We treat the
/// presence of a *recently modified* lockfile as "in use" and refuse to delete.
/// This is conservative on purpose — a false "in use" just defers cleanup; a
/// false "safe" could corrupt a running install.
pub fn lock_present(dir: &Path) -> bool {
    for name in [".lock", "lock", ".package-lock"] {
        let lp = dir.join(name);
        if lp.exists() {
            // Only treat as active if touched in the last 5 minutes; stale locks
            // from a crashed process shouldn't block cleanup forever.
            if let Ok(meta) = fs::metadata(&lp) {
                if let Ok(m) = meta.modified() {
                    if let Ok(age) = SystemTime::now().duration_since(m) {
                        if age.as_secs() < 300 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Find directories named `name` under `root`, pruning at first match so we do
/// not descend into nested matches (node_modules inside node_modules).
pub fn find_named_dirs(root: &Path, name: &str, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let p = entry.path();
        if entry.file_name() == name {
            out.push(p); // matched — do not descend
        } else {
            find_named_dirs(&p, name, out);
        }
    }
}

/// Seconds since epoch, now. Wrapper so callers stay testable.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
