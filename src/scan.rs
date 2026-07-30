//! Directory sizing and lock detection.
//!
//! Sizing uses jwalk, which parallelises the walk. That helped (a 193k-file cache
//! went from 8.7s to ~2.4s against the old hand-rolled recursion) but it is not
//! where the remaining time goes: `bench/bench.sh` shows BSD `du` finishing the
//! same tree ~11x faster than this, and capping jwalk's thread pool changes
//! nothing. The gap is syscall strategy — `du` uses fts(3) to stream directory
//! entries, while this does a stat per file. An fts-style batched reader is the
//! open optimisation; see the Benchmarks section of the README.
//!
//! Two settings here are load-bearing rather than incidental:
//! `follow_links(false)` keeps a symlinked cache from being used to size (and
//! later delete) something outside its root, and `skip_hidden(false)` is required
//! because nearly every target is a dotfile — `.cache`, `.npm`, `.venv`. Getting
//! that second one wrong silently reports 0 B for everything.

use jwalk::WalkDir;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DirStat {
    pub bytes: u64,
    pub files: u64,
    /// Most-recent mtime seen, as seconds since epoch. Used for age-gating.
    pub newest_mtime: u64,
}

/// Recursively size a directory. Symlinks are counted as an entry but never
/// traversed, so we can't be tricked into sizing (or deleting) outside `path`.
pub fn size_dir(path: &Path) -> DirStat {
    let mut stat = DirStat {
        bytes: 0,
        files: 0,
        newest_mtime: 0,
    };
    for entry in WalkDir::new(path)
        .follow_links(false) // security: never escape the target via a symlink
        .skip_hidden(false) // cache dirs are dotfiles; skipping them reports 0 B
        .into_iter()
        .flatten()
    {
        // Branch on the file_type jwalk captured during readdir. Calling
        // entry.metadata() here instead would issue a second stat for every
        // entry, including directories we don't measure — 200k redundant
        // syscalls on a cache-sized tree.
        let ft = entry.file_type();
        if ft.is_symlink() {
            stat.files += 1;
            continue;
        }
        if ft.is_dir() {
            continue; // directory inodes themselves add no meaningful bytes
        }
        // Only regular files need a stat, for length and mtime.
        let Ok(meta) = entry.metadata() else { continue };
        stat.bytes += meta.len();
        stat.files += 1;
        if let Ok(m) = meta.modified() {
            if let Ok(d) = m.duration_since(UNIX_EPOCH) {
                let secs = d.as_secs();
                if secs > stat.newest_mtime {
                    stat.newest_mtime = secs;
                }
            }
        }
    }
    stat
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
