//! cachewipe — fast, safe reclaimer of regenerable cache and build files.
//!
//! Reports by default. Deletes only with --apply. The safety-critical decisions
//! (what may be touched, whether a path is in-bounds, whether it is in use) live
//! in tested modules, not in this glue.

mod safety;
mod scan;
mod targets;

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
    total_reclaimable_bytes: u64,
    total_deleted_bytes: u64,
    items: Vec<Item>,
}

struct Config {
    apply: bool,
    include_os_caches: bool,
    min_age_days: u64,
    scan_roots: Vec<PathBuf>,
    json: bool,
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config {
        apply: false,
        include_os_caches: false,
        min_age_days: 0,
        scan_roots: Vec::new(),
        json: false,
    };
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--apply" => cfg.apply = true,
            "--include-os-caches" => cfg.include_os_caches = true,
            "--json" => cfg.json = true,
            "--min-age-days" => {
                let v = args.next().ok_or("--min-age-days needs a value")?;
                cfg.min_age_days = v
                    .parse()
                    .map_err(|_| "invalid --min-age-days".to_string())?;
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
         USAGE:\n  cachewipe [--apply] [--json] [--include-os-caches] [--min-age-days N] [--root PATH]...\n\n\
         By default cachewipe REPORTS what it would free and deletes NOTHING.\n\
         Pass --apply to actually delete. OS/app caches are excluded unless\n\
         --include-os-caches is given. --root adds a projects dir to scan for\n\
         build artifacts (node_modules, .venv, target, .next, __pycache__).\n\n\
         Exit code 0 = success. Machine-readable output with --json."
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

    let report = run(&cfg, &home, &home_path);

    if cfg.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        print_human(&report);
    }
}

fn run(cfg: &Config, home: &str, home_path: &Path) -> Report {
    let mut items = Vec::new();
    let now = scan::now_secs();
    let min_age_secs = cfg.min_age_days.saturating_mul(86_400);

    for target in targets::catalog() {
        // Tier gating: OS caches off unless opted in; others default-on.
        let included = match target.tier {
            Tier::OsCache => cfg.include_os_caches,
            _ => target.tier.default_on(),
        };
        if !included {
            continue;
        }

        match target.kind {
            Kind::HomeDir(sub) => {
                let path = targets::home_path(home, sub);
                push_dir_item(
                    &mut items,
                    cfg,
                    &target,
                    &path,
                    &path,
                    home_path,
                    now,
                    min_age_secs,
                );
            }
            Kind::NamedDirUnder { name } => {
                // Build artifacts only scanned if the user gave --root(s).
                for root in &cfg.scan_roots {
                    let mut found = Vec::new();
                    scan::find_named_dirs(root, name, &mut found);
                    for p in found {
                        push_dir_item(
                            &mut items,
                            cfg,
                            &target,
                            &p,
                            root,
                            home_path,
                            now,
                            min_age_secs,
                        );
                    }
                }
            }
            Kind::External { probe } => {
                handle_docker(&mut items, cfg, &target, probe);
            }
        }
    }

    let total_reclaimable_bytes = items
        .iter()
        .filter(|i| i.verdict == "reclaimable" || i.reason == "deleted")
        .map(|i| i.bytes)
        .sum();
    let total_deleted_bytes = items
        .iter()
        .filter(|i| i.reason == "deleted")
        .map(|i| i.bytes)
        .sum();

    Report {
        dry_run: !cfg.apply,
        total_reclaimable_bytes,
        total_deleted_bytes,
        items,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_dir_item(
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
        let s = scan::size_dir(path);
        (s.bytes, s.files, s.newest_mtime)
    } else {
        (0, 0, 0)
    };

    // Age gate: skip dirs whose newest file is younger than the threshold.
    let too_new = min_age_secs > 0 && newest > 0 && now.saturating_sub(newest) < min_age_secs;

    let (v, reason) = match verdict {
        Guard::Missing => ("skipped", "not present".to_string()),
        Guard::Protected => ("skipped", "protected path — refused".to_string()),
        Guard::OutsideRoot => ("skipped", "outside allowed root — refused".to_string()),
        Guard::InUse => ("skipped", "in use (active lock) — refused".to_string()),
        Guard::Ok if too_new => ("skipped", format!("newer than {} days", cfg.min_age_days)),
        Guard::Ok => {
            if cfg.apply {
                match std::fs::remove_dir_all(path) {
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

/// Docker is delegated to the engine — we NEVER rm docker's files ourselves.
/// Dry-run reports reclaimable via `docker system df`; --apply runs a scoped
/// prune of dangling images + build cache (not -a, not --volumes).
fn handle_docker(items: &mut Vec<Item>, cfg: &Config, target: &targets::Target, probe: &str) {
    let docker_ok = Command::new(probe)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !docker_ok {
        items.push(Item {
            id: target.id.to_string(),
            tier: target.tier.as_str().to_string(),
            path: "docker daemon".to_string(),
            bytes: 0,
            files: 0,
            regenerates: target.regenerates.to_string(),
            verdict: "skipped".to_string(),
            reason: "docker not installed".to_string(),
        });
        return;
    }

    let reason = if cfg.apply {
        let out = Command::new(probe)
            .args(["system", "prune", "-f"]) // dangling images + build cache; NOT -a/--volumes
            .output();
        match out {
            Ok(o) if o.status.success() => "pruned (dangling images + build cache)".to_string(),
            Ok(o) => format!(
                "docker prune failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => format!("docker prune error: {e}"),
        }
    } else {
        "dry-run — run `docker system df` to size; apply prunes dangling only".to_string()
    };

    items.push(Item {
        id: target.id.to_string(),
        tier: target.tier.as_str().to_string(),
        path: "docker (dangling images + build cache)".to_string(),
        bytes: 0, // docker reports its own sizes; we don't double-count
        files: 0,
        regenerates: target.regenerates.to_string(),
        verdict: "reclaimable".to_string(),
        reason,
    });
}

fn print_human(r: &Report) {
    println!(
        "cachewipe {}",
        if r.dry_run {
            "(dry-run — nothing deleted)"
        } else {
            "(APPLY — deleting)"
        }
    );
    println!("{:-<64}", "");
    for i in &r.items {
        if i.verdict == "reclaimable" {
            println!(
                "  {:>8}  {:<16} {}  [{}]",
                human(i.bytes),
                i.id,
                i.path,
                i.reason
            );
        } else {
            println!("  {:>8}  {:<16} {}  ({})", "skip", i.id, i.path, i.reason);
        }
    }
    println!("{:-<64}", "");
    if r.dry_run {
        println!(
            "Reclaimable: {}   (run again with --apply to delete)",
            human(r.total_reclaimable_bytes)
        );
    } else {
        println!("Deleted: {}", human(r.total_deleted_bytes));
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
}
