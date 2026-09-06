//! cachewipe — fast, safe reclaimer of regenerable cache and build files.
//!
//! Reports by default. Deletes only with --apply. The safety-critical decisions
//! (what may be touched, whether a path is in-bounds, whether it is in use) live
//! in tested modules, not in this glue.

mod safety;
mod scan;
mod targets;

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use safety::Guard;
use targets::{Kind, Tier};

#[derive(serde::Serialize)]
struct Item {
    id: String,
    tier: String,
    path: String,
    bytes: u64,
    files: u64,
    regenerates: String,
    verdict: String, // "reclaimable" | "skipped"
    reason: String,  // why skipped, or "dry-run" / "deleted"
}

#[derive(serde::Serialize)]
struct Report {
    dry_run: bool,
    /// What --apply would (or did) free with the current flags.
    total_reclaimable_bytes: u64,
    /// Sized and shown, but held back behind an opt-in flag. Kept separate so
    /// the headline number never promises space the current flags won't free.
    total_opt_in_bytes: u64,
    total_deleted_bytes: u64,
    /// Free space on the data volume, before and after. `after` is only
    /// meaningful under --apply.
    free_bytes_before: Option<u64>,
    free_bytes_after: Option<u64>,
    items: Vec<Item>,
}

struct Config {
    apply: bool,
    include_os_caches: bool,
    include_vm_disks: bool,
    min_age_days: u64,
    scan_roots: Vec<PathBuf>,
    json: bool,
    top: usize,
    /// Skip delegated cleanups entirely.
    ///
    /// Every other target is a path under `$HOME`, so pointing `$HOME` at a
    /// scratch directory fully sandboxes the run. Delegated cleanups break that
    /// property: they are subprocesses that consult their own config, so
    /// `docker system prune` and `brew cleanup` reach the real machine no matter
    /// what `$HOME` says. Anyone testing against a fake home needs a way to turn
    /// them off, or the "sandbox" quietly isn't one.
    no_external: bool,
}

/// A resolved deletion candidate, before it has been sized or judged.
/// Collecting these first is what lets us drop nested duplicates before any
/// byte is counted or removed.
struct Candidate {
    target_idx: usize,
    path: PathBuf,
    root: PathBuf,
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config {
        apply: false,
        include_os_caches: false,
        include_vm_disks: false,
        min_age_days: 0,
        scan_roots: Vec::new(),
        json: false,
        top: 12,
        no_external: false,
    };
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--apply" => cfg.apply = true,
            "--include-os-caches" => cfg.include_os_caches = true,
            "--include-vm-disks" => cfg.include_vm_disks = true,
            "--no-external" => cfg.no_external = true,
            "--json" => cfg.json = true,
            "--min-age-days" => {
                let v = args.next().ok_or("--min-age-days needs a value")?;
                cfg.min_age_days = v
                    .parse()
                    .map_err(|_| "invalid --min-age-days".to_string())?;
            }
            "--top" => {
                let v = args.next().ok_or("--top needs a value")?;
                cfg.top = v.parse().map_err(|_| "invalid --top".to_string())?;
            }
            "--root" => {
                let v = args.next().ok_or("--root needs a path")?;
                cfg.scan_roots.push(PathBuf::from(v));
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(cfg)
}

fn print_help() {
    println!(
        "cachewipe — reclaim regenerable cache & build files (safe by default)\n\n\
         USAGE:\n  cachewipe [--apply] [--json] [--top N] [--include-os-caches]\n            [--include-vm-disks] [--min-age-days N] [--root PATH]...\n\n\
         By default cachewipe REPORTS what it would free and deletes NOTHING.\n\
         Pass --apply to actually delete.\n\n\
         --root PATH           scan a projects dir for build artifacts\n\
         \x20                     (node_modules, .venv, target, .next, __pycache__)\n\
         --include-os-caches   also delete named app caches under ~/Library/Caches\n\
         --include-vm-disks    also delete container VM disk images. These are\n\
         \x20                     always REPORTED because they are usually the\n\
         \x20                     biggest item on the disk, but deleting one\n\
         \x20                     destroys every local image, container and named\n\
         \x20                     volume for that engine, so it needs this flag.\n\
         --min-age-days N      only touch things whose newest file is older than N\n\
         --top N               how many individual paths to list (default 12)\n\
         --no-external         skip delegated cleanups (docker prune, brew cleanup).\n\
         \x20                     Those are subprocesses that read their own config,\n\
         \x20                     so they reach the real machine even when $HOME is\n\
         \x20                     pointed at a scratch dir. Pass this whenever you\n\
         \x20                     are testing against a fake home.\n\n\
         Exit code 0 = success. Machine-readable output with --json:\n\
         {{ dry_run, total_reclaimable_bytes, total_opt_in_bytes,\n\
         \x20 total_deleted_bytes, free_bytes_before, free_bytes_after,\n\
         \x20 items: [{{ id, tier, path, bytes, files, regenerates, verdict, reason }}] }}"
    );
}

fn main() {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}\n");
            print_help();
            std::process::exit(2);
        }
    };

    let home = env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        eprintln!("error: $HOME is not set; refusing to run without a home anchor");
        std::process::exit(2);
    }
    let home_path = PathBuf::from(&home);

    // Warn at the only moment this can actually bite: a mutating run whose
    // $HOME has been redirected. Every path target is then sandboxed, but the
    // delegated cleanups are not, so the operator is about to prune the real
    // machine while believing they are in a scratch dir.
    if cfg.apply && !cfg.no_external && home_is_overridden(&home) {
        eprintln!(
            "warning: $HOME is {home}, which is not this user's login home.\n\
             \x20        Path targets are confined to it, but delegated cleanups\n\
             \x20        (docker prune, brew cleanup) are subprocesses that read\n\
             \x20        their own config and will affect the REAL machine.\n\
             \x20        Pass --no-external to keep this run inside the sandbox."
        );
    }

    let report = run(&cfg, &home, &home_path);

    if cfg.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        print_human(&report, cfg.top);
    }
}

/// Does `$HOME` look redirected away from this user's login home?
///
/// Deliberately a convention check rather than a passwd lookup: reading the
/// password database would mean a new dependency for a hint, and the cost of
/// being wrong is only a warning either way. Unknown `$USER` means no opinion,
/// so unusual setups stay quiet instead of crying wolf.
fn home_is_overridden(home: &str) -> bool {
    let Ok(user) = env::var("USER") else {
        return false;
    };
    if user.is_empty() {
        return false;
    }
    let conventional = [
        format!("/Users/{user}"),
        format!("/home/{user}"),
        "/var/root".to_string(),
        "/root".to_string(),
    ];
    !conventional.iter().any(|c| c == home)
}

/// Is this tier deletable under the current flags?
fn deletable(tier: Tier, cfg: &Config) -> bool {
    match tier {
        Tier::OsCache => cfg.include_os_caches,
        Tier::VmDisk => cfg.include_vm_disks,
        t => t.default_on(),
    }
}

fn run(cfg: &Config, home: &str, home_path: &Path) -> Report {
    let catalog = targets::catalog();
    let vol = scan::data_volume();
    let free_before = scan::free_bytes(&vol);

    // --- Phase 1: resolve every candidate path, without sizing or deleting ---
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut items = Vec::new();

    for (idx, target) in catalog.iter().enumerate() {
        // A tier we can neither delete nor are asked to report is skipped whole.
        if !deletable(target.tier, cfg) && !target.tier.always_reported() {
            continue;
        }

        match target.kind {
            Kind::HomeDirs(subs) => {
                for sub in subs {
                    let path = targets::home_path(home, sub);
                    // Report a missing path only if no sibling location matched,
                    // otherwise a multi-platform target logs noise on every run.
                    candidates.push(Candidate {
                        target_idx: idx,
                        root: path.clone(),
                        path,
                    });
                }
            }
            Kind::VersionedDirs { subpaths, keep } => {
                for sub in subpaths {
                    let dir = targets::home_path(home, sub);
                    if !dir.exists() {
                        continue;
                    }
                    let mut stale = Vec::new();
                    scan::stale_versions(&dir, keep, &mut stale);
                    for p in stale {
                        candidates.push(Candidate {
                            target_idx: idx,
                            root: dir.clone(),
                            path: p,
                        });
                    }
                }
            }
            Kind::NamedDirUnder {
                name,
                requires_sibling,
            } => {
                for root in &cfg.scan_roots {
                    let mut found = Vec::new();
                    scan::find_named_dirs(root, name, requires_sibling, &mut found);
                    for p in found {
                        candidates.push(Candidate {
                            target_idx: idx,
                            root: root.clone(),
                            path: p,
                        });
                    }
                }
            }
            Kind::External { .. } => { /* handled after path candidates */ }
        }
    }

    // --- Phase 2: drop nested candidates ---
    let kept = dedupe_nested(candidates);

    // --- Phase 3: size, judge, and (with --apply) delete ---
    let now = scan::now_secs();
    let min_age_secs = cfg.min_age_days.saturating_mul(86_400);
    let mut seen_present: BTreeMap<usize, bool> = BTreeMap::new();

    for c in &kept {
        let target = &catalog[c.target_idx];
        let present = c.path.exists();
        seen_present
            .entry(c.target_idx)
            .and_modify(|v| *v |= present)
            .or_insert(present);
        push_item(
            &mut items,
            cfg,
            target,
            &c.path,
            &c.root,
            home_path,
            now,
            min_age_secs,
        );
    }

    // A HomeDirs target with several platform locations should not report
    // "not present" for the ones that don't apply once another matched.
    for (idx, present) in &seen_present {
        if *present {
            let id = catalog[*idx].id;
            items.retain(|i| !(i.id == id && i.reason == "not present"));
        }
    }

    for (idx, target) in catalog.iter().enumerate() {
        if let Kind::External {
            probe,
            apply_args,
            note,
        } = target.kind
        {
            let _ = idx;
            if cfg.no_external || !deletable(target.tier, cfg) {
                continue;
            }
            handle_external(&mut items, cfg, target, probe, apply_args, note);
        }
    }

    let total_reclaimable_bytes = items
        .iter()
        .filter(|i| i.verdict == "reclaimable" || i.reason == "deleted")
        .map(|i| i.bytes)
        .sum();
    let total_opt_in_bytes = items
        .iter()
        .filter(|i| i.reason.starts_with("needs --"))
        .map(|i| i.bytes)
        .sum();
    let total_deleted_bytes = items
        .iter()
        .filter(|i| i.reason == "deleted")
        .map(|i| i.bytes)
        .sum();

    let free_after = if cfg.apply {
        scan::free_bytes(&vol)
    } else {
        None
    };

    Report {
        dry_run: !cfg.apply,
        total_reclaimable_bytes,
        total_opt_in_bytes,
        total_deleted_bytes,
        free_bytes_before: free_before,
        free_bytes_after: free_after,
        items,
    }
}

/// Drop any candidate that lives inside another candidate.
///
/// Targets are resolved independently, so nesting across them is normal:
/// `.next/standalone/frontend/node_modules` is found by the node_modules target
/// while `.next` is found by the next target, and `__pycache__` dirs turn up
/// inside an already-matched `.venv`. Counting both inflates the total and
/// double-reports the same bytes. The outer path wins — deleting it removes the
/// inner one anyway.
fn dedupe_nested(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    let mut kept: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for c in candidates {
        let nested_in_kept = kept
            .last()
            .map(|prev| c.path.starts_with(&prev.path))
            .unwrap_or(false);
        if nested_in_kept {
            continue;
        }
        // Sorted order only guarantees the immediate predecessor is a prefix
        // candidate when it is itself kept, so re-check against all kept
        // ancestors cheaply by walking parents.
        if kept
            .iter()
            .any(|prev| c.path != prev.path && c.path.starts_with(&prev.path))
        {
            continue;
        }
        kept.push(c);
    }
    kept
}

#[allow(clippy::too_many_arguments)]
fn push_item(
    items: &mut Vec<Item>,
    cfg: &Config,
    target: &targets::Target,
    path: &Path,
    root: &Path,
    home: &Path,
    now: u64,
    min_age_secs: u64,
) {
    let exists = path.exists();
    let locked = exists && scan::lock_present(path);
    let verdict = safety::evaluate(path, root, home, exists, locked);

    let (bytes, files, newest) = if exists {
        let s = scan::size_path(path);
        (s.bytes, s.files, s.newest_mtime)
    } else {
        (0, 0, 0)
    };

    let too_new = min_age_secs > 0 && newest > 0 && now.saturating_sub(newest) < min_age_secs;
    let gated = !deletable(target.tier, cfg);

    let (v, reason) = match verdict {
        Guard::Missing => ("skipped", "not present".to_string()),
        Guard::Protected => ("skipped", "protected path — refused".to_string()),
        Guard::OutsideRoot => ("skipped", "outside allowed root — refused".to_string()),
        Guard::InUse => ("skipped", "in use (active lock) — refused".to_string()),
        // Sized and shown, but this tier needs an explicit opt-in to delete.
        Guard::Ok if gated => (
            "skipped",
            match target.tier {
                Tier::VmDisk => "needs --include-vm-disks".to_string(),
                _ => "needs --include-os-caches".to_string(),
            },
        ),
        Guard::Ok if too_new => ("skipped", format!("newer than {} days", cfg.min_age_days)),
        Guard::Ok => {
            if cfg.apply {
                match remove_path(path) {
                    Ok(_) => ("reclaimable", "deleted".to_string()),
                    Err(e) => ("skipped", format!("delete failed: {e}")),
                }
            } else {
                ("reclaimable", "dry-run".to_string())
            }
        }
    };

    items.push(Item {
        id: target.id.to_string(),
        tier: target.tier.as_str().to_string(),
        path: path.display().to_string(),
        bytes,
        files,
        regenerates: target.regenerates.to_string(),
        verdict: v.to_string(),
        reason,
    });
}

/// Remove a candidate, which may be a directory or a single file. VM disks are
/// files, so a dir-only remove silently failed on the biggest targets.
fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Delegated cleanup: we run the tool's own command rather than removing its
/// files. Sizes are left to the tool, and `note` states what the command does
/// NOT cover — for Docker that gap (prune does not shrink the VM disk) is the
/// difference between a report the user can trust and one they can't.
fn handle_external(
    items: &mut Vec<Item>,
    cfg: &Config,
    target: &targets::Target,
    probe: &str,
    apply_args: &[&str],
    note: &str,
) {
    let available = Command::new(probe)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !available {
        items.push(Item {
            id: target.id.to_string(),
            tier: target.tier.as_str().to_string(),
            path: format!("{probe} (not installed)"),
            bytes: 0,
            files: 0,
            regenerates: target.regenerates.to_string(),
            verdict: "skipped".to_string(),
            reason: format!("{probe} not installed"),
        });
        return;
    }

    let reason = if cfg.apply {
        match Command::new(probe).args(apply_args).output() {
            Ok(o) if o.status.success() => format!("ran `{probe} {}`", apply_args.join(" ")),
            Ok(o) => format!(
                "`{probe} {}` failed: {}",
                apply_args.join(" "),
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => format!("`{probe}` error: {e}"),
        }
    } else {
        format!("dry-run — would run `{probe} {}`", apply_args.join(" "))
    };

    items.push(Item {
        id: target.id.to_string(),
        tier: target.tier.as_str().to_string(),
        path: format!("{probe}: {note}"),
        // Unsized on purpose: the engine owns these numbers. Where a real file
        // backs the space (a VM disk) it has its own catalog entry that IS sized.
        bytes: 0,
        files: 0,
        regenerates: target.regenerates.to_string(),
        verdict: "reclaimable".to_string(),
        reason,
    });
}

fn print_human(r: &Report, top: usize) {
    println!(
        "cachewipe {}",
        if r.dry_run {
            "(dry-run — nothing deleted)"
        } else {
            "(APPLY — deleting)"
        }
    );
    if let Some(before) = r.free_bytes_before {
        print!("free on data volume: {}", human(before));
        match r.free_bytes_after {
            Some(after) => println!(" → {}", human(after)),
            None => println!(),
        }
    }
    println!("{:-<72}", "");

    // Rollup by target id. A raw per-path list is unreadable on a real machine —
    // one run produced 486 lines, mostly __pycache__ — so lead with the totals
    // that drive a decision and cap the path list.
    let mut by_id: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
    for i in &r.items {
        if i.verdict == "reclaimable" {
            let e = by_id.entry(&i.id).or_insert((0, 0));
            e.0 += i.bytes;
            e.1 += 1;
        }
    }
    let mut rows: Vec<_> = by_id.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1 .0));
    for (id, (bytes, n)) in &rows {
        let count = if *n > 1 {
            format!("  ({n} paths)")
        } else {
            String::new()
        };
        println!("  {:>9}  {:<20}{}", human(*bytes), id, count);
    }

    // Largest individual paths, so a surprising entry is still visible.
    let mut big: Vec<&Item> = r
        .items
        .iter()
        .filter(|i| i.verdict == "reclaimable" && i.bytes > 0)
        .collect();
    big.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    if !big.is_empty() {
        println!("\n  largest:");
        for i in big.iter().take(top) {
            println!("    {:>9}  {}", human(i.bytes), i.path);
        }
        if big.len() > top {
            println!("    … and {} more (--top N, or --json)", big.len() - top);
        }
    }

    // Opt-in items: shown with their size because that size is usually the
    // reason someone runs this tool at all.
    let gated: Vec<&Item> = r
        .items
        .iter()
        .filter(|i| i.reason.starts_with("needs --"))
        .collect();
    if !gated.is_empty() {
        println!("\n  held back (needs a flag):");
        for i in &gated {
            println!("    {:>9}  {:<20} {}", human(i.bytes), i.id, i.reason);
        }
    }

    // Everything else that was skipped, minus the "not present" noise.
    let skipped: Vec<&Item> = r
        .items
        .iter()
        .filter(|i| {
            i.verdict == "skipped" && i.reason != "not present" && !i.reason.starts_with("needs --")
        })
        .collect();
    if !skipped.is_empty() {
        println!("\n  skipped:");
        for i in &skipped {
            println!("    {:<20} {}  ({})", i.id, i.path, i.reason);
        }
    }
    let absent = r.items.iter().filter(|i| i.reason == "not present").count();
    if absent > 0 {
        println!("\n  {absent} target(s) not present on this machine");
    }

    println!("{:-<72}", "");
    if r.dry_run {
        println!(
            "Reclaimable: {}   (run again with --apply to delete)",
            human(r.total_reclaimable_bytes)
        );
    } else {
        println!("Deleted: {}", human(r.total_deleted_bytes));
    }
    if r.total_opt_in_bytes > 0 {
        println!(
            "Held back: {}   (opt in with --include-vm-disks / --include-os-caches)",
            human(r.total_opt_in_bytes)
        );
    }
}

fn human(bytes: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut b = bytes as f64;
    let mut i = 0;
    while b >= 1024.0 && i < U.len() - 1 {
        b /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}{}", bytes, U[0])
    } else {
        format!("{:.1}{}", b, U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_readable_sizes() {
        assert_eq!(human(0), "0B");
        assert_eq!(human(512), "512B");
        assert_eq!(human(1024), "1.0KB");
        assert_eq!(human(1_073_741_824), "1.0GB");
    }

    fn cand(p: &str) -> Candidate {
        Candidate {
            target_idx: 0,
            path: PathBuf::from(p),
            root: PathBuf::from("/r"),
        }
    }

    #[test]
    fn dedupe_drops_nested_paths() {
        // node_modules inside an already-matched .next must not be counted twice.
        let kept = dedupe_nested(vec![
            cand("/r/app/.next"),
            cand("/r/app/.next/standalone/node_modules"),
            cand("/r/app/node_modules"),
        ]);
        let paths: Vec<String> = kept.iter().map(|c| c.path.display().to_string()).collect();
        assert_eq!(paths, vec!["/r/app/.next", "/r/app/node_modules"]);
    }

    #[test]
    fn dedupe_keeps_siblings_with_shared_prefix_text() {
        // "/r/a" must not swallow "/r/ab" — prefix matching is per-component.
        let kept = dedupe_nested(vec![cand("/r/a"), cand("/r/ab")]);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn dedupe_drops_deeply_nested_not_just_immediate() {
        let kept = dedupe_nested(vec![
            cand("/r/v/.venv"),
            cand("/r/v/.venv/lib/python3.12/site-packages/x/__pycache__"),
            cand("/r/v/.venv/lib/python3.12/site-packages/y/__pycache__"),
        ]);
        assert_eq!(kept.len(), 1);
    }
}
