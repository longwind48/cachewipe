//! Directory sizing and lock detection.
//!
//! Sizing is a serial depth-first walk, and that is a measured choice rather than
//! a lazy one. This code was briefly parallelised with jwalk; `bench/bench.sh`
//! showed the parallel version was ~4.8x SLOWER on a 200k-file cache. Isolating
//! the stages explains why: jwalk's enumeration alone is very fast (96ms vs 383ms
//! for `du`), but a parallel walk visits directories out of order, so the stat of
//! each file misses the metadata locality a depth-first walk keeps warm. Feeding
//! jwalk's output into a serial stat loop was still 4.75x slower than walking
//! serially, which rules out the stat call and jwalk's own metadata() as the
//! cause — it is the traversal order.
//!
//! Before optimising this, read the Benchmarks section of the README and rerun
//! bench/bench.sh. The obvious idea (add threads) has been tried and lost.
//!
//! Symlinks are never traversed, which keeps a symlinked cache from being used to
//! size — and later delete — something outside its root.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DirStat {
    pub bytes: u64,
    pub files: u64,
    /// Most-recent mtime seen, as seconds since epoch. Used for age-gating.
    pub newest_mtime: u64,
}

/// Size a path that may be either a directory or a single file.
///
/// The file case is not hypothetical: a container VM's disk is one enormous
/// file (Docker Desktop's `Docker.raw`), and the directory-only version of this
/// function returned 0 bytes for it — reporting the single largest reclaimable
/// object on the machine as empty.
pub fn size_path(path: &Path) -> DirStat {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return DirStat {
            bytes: 0,
            files: 0,
            newest_mtime: 0,
        };
    };
    if meta.is_dir() {
        return size_dir(path);
    }
    let newest = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    DirStat {
        // A sparse VM disk reports a huge apparent length; blocks*512 is what
        // the filesystem actually gives back, which is the number the user
        // cares about. Fall back to len() where blocks are unavailable.
        bytes: allocated_bytes(&meta).unwrap_or(meta.len()),
        files: 1,
        newest_mtime: newest,
    }
}

#[cfg(unix)]
fn allocated_bytes(meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.blocks().saturating_mul(512))
}

#[cfg(not(unix))]
fn allocated_bytes(_meta: &fs::Metadata) -> Option<u64> {
    None
}

/// Recursively size a directory. Symlinks are counted as an entry but never
/// traversed, so we can't be tricked into sizing (or deleting) outside `path`.
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
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        // DirEntry::metadata() does not follow symlinks — this is the lstat.
        let Ok(meta) = entry.metadata() else { continue };
        let ft = meta.file_type();
        if ft.is_symlink() {
            acc.files += 1;
            continue; // never traverse symlinks
        }
        if ft.is_dir() {
            size_into(&entry.path(), acc);
            continue;
        }
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
///
/// `requires_sibling`, when non-empty, means the name alone is not enough
/// evidence that this is build output — one of those files must sit beside the
/// directory. `target/` is the motivating case: it is Cargo's build dir, and
/// also an ordinary data directory name. A name-only match there deletes real
/// data that no build regenerates. When the manifest is absent we keep walking
/// into the directory rather than pruning, so a genuine project nested inside
/// someone's data tree is still found.
pub fn find_named_dirs(root: &Path, name: &str, requires_sibling: &[&str], out: &mut Vec<PathBuf>) {
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
            if requires_sibling.is_empty() || has_sibling(&p, requires_sibling) {
                out.push(p); // matched — do not descend
                continue;
            }
            // Named right but unproven: treat as an ordinary directory.
            find_named_dirs(&p, name, requires_sibling, out);
        } else {
            find_named_dirs(&p, name, requires_sibling, out);
        }
    }
}

/// Does one of `names` exist next to `dir` (i.e. in its parent)?
pub fn has_sibling(dir: &Path, names: &[&str]) -> bool {
    let Some(parent) = dir.parent() else {
        return false;
    };
    names.iter().any(|n| parent.join(n).exists())
}

/// Versioned installs under `dir` that are safe to reclaim: every child except
/// the newest `keep` of each product group.
///
/// Children are grouped by `targets::version_group` so a mixed directory stays
/// correct — chromium and firefox each keep their own newest rather than the
/// single newest overall. Ordering is by directory mtime, which is install time
/// in practice and avoids parsing the many version-string dialects these tools
/// use. Ties break on name so the result is deterministic.
pub fn stale_versions(dir: &Path, keep: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    // group -> Vec<(mtime, name, path)>
    let mut groups: std::collections::HashMap<String, Vec<(u64, String, PathBuf)>> =
        std::collections::HashMap::new();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let group = crate::targets::version_group(&name).to_string();
        groups
            .entry(group)
            .or_default()
            .push((mtime, name, entry.path()));
    }
    for (_, mut versions) in groups {
        if versions.len() <= keep {
            continue;
        }
        // Newest first, then reclaim everything past `keep`.
        versions.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        for (_, _, p) in versions.into_iter().skip(keep) {
            out.push(p);
        }
    }
    out.sort();
}

/// Free bytes on the filesystem holding `path`, via `df -k`.
///
/// On modern macOS `df /` reports the sealed, read-only system volume, which is
/// always nearly full and has nothing to do with reclaimable space. The number
/// a user means by "free disk" lives on the data volume, so callers should pass
/// `data_volume()`. Getting this wrong makes a cleanup tool confidently report
/// the wrong headline number.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let out = Command::new("df").arg("-k").arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Last line, 4th whitespace field is Avail in 1K blocks.
    let line = text.lines().last()?;
    let avail_k: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_k.saturating_mul(1024))
}

/// The volume whose free space the user actually means.
pub fn data_volume() -> PathBuf {
    let macos_data = Path::new("/System/Volumes/Data");
    if macos_data.exists() {
        return macos_data.to_path_buf();
    }
    PathBuf::from("/")
}

/// Seconds since epoch, now. Wrapper so callers stay testable.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
