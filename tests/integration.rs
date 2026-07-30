//! End-to-end tests against a real (temporary) filesystem.
//!
//! We build a fake $HOME with a fake cache dir, run the compiled binary, and
//! assert on its JSON output and on what actually survived on disk. This is the
//! test that matters most: it proves "dry-run deletes nothing" and "--apply
//! deletes only what it claimed" without trusting any single unit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Minimal temp-dir helper (no external crates). Unique per test via the test
/// name passed in, plus the process id, so parallel tests don't collide.
fn scratch(tag: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    d.push(format!("cachewipe-it-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write_file(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cachewipe")
}

fn run(home: &Path, extra: &[&str]) -> String {
    let out = Command::new(bin())
        .env("HOME", home)
        .arg("--json")
        .args(extra)
        .output()
        .expect("run cachewipe");
    assert!(
        out.status.success(),
        "nonzero exit: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn dry_run_deletes_nothing() {
    let home = scratch("dryrun");
    let uv = home.join(".cache/uv/archive/pkg");
    write_file(&uv.join("wheel.bin"), 4096);

    let json = run(&home, &[]);
    assert!(json.contains("\"dry_run\": true"));
    assert!(json.contains("\"id\": \"uv\""));
    assert!(json.contains("\"reason\": \"dry-run\""));
    // The file must still exist — dry-run is non-destructive.
    assert!(uv.join("wheel.bin").exists(), "dry-run must not delete");

    fs::remove_dir_all(&home).ok();
}

#[test]
fn apply_deletes_the_cache() {
    let home = scratch("apply");
    let uv = home.join(".cache/uv/archive/pkg");
    write_file(&uv.join("wheel.bin"), 4096);
    assert!(uv.exists());

    let json = run(&home, &["--apply"]);
    assert!(json.contains("\"dry_run\": false"));
    assert!(json.contains("\"reason\": \"deleted\""));
    // uv cache dir should be gone.
    assert!(
        !home.join(".cache/uv").exists(),
        "--apply must delete the cache"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn os_cache_off_by_default_on_by_flag() {
    let home = scratch("oscache");
    write_file(&home.join("Library/Caches/App/blob.bin"), 2048);

    // Default: os-user-cache present but skipped as "not included" (absent from items or skipped).
    let default_json = run(&home, &[]);
    // It should not be reported as reclaimable without the opt-in flag.
    assert!(
        !default_json.contains("\"id\": \"os-user-cache\"")
            || default_json.contains("os-user-cache")
                && !reclaimable_for(&default_json, "os-user-cache"),
        "os cache must not be reclaimable by default"
    );

    // Opt-in: now it should appear as reclaimable.
    let opt_json = run(&home, &["--include-os-caches"]);
    assert!(opt_json.contains("\"id\": \"os-user-cache\""));
    assert!(
        reclaimable_for(&opt_json, "os-user-cache"),
        "opt-in should surface os cache"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn active_lock_blocks_deletion() {
    let home = scratch("lock");
    let uv = home.join(".cache/uv");
    write_file(&uv.join("archive/wheel.bin"), 4096);
    // A fresh .lock at the cache root => "in use".
    write_file(&uv.join(".lock"), 0);

    let json = run(&home, &["--apply"]);
    assert!(
        json.contains("in use"),
        "locked cache must be refused; got: {json}"
    );
    // Nothing deleted because the only target was locked.
    assert!(
        uv.join("archive/wheel.bin").exists(),
        "locked cache must survive"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn build_artifacts_only_scanned_with_root() {
    let home = scratch("artifacts");
    let proj = home.join("code/myapp");
    write_file(&proj.join("node_modules/dep/index.js"), 1024);

    // Without --root: node_modules not scanned.
    let no_root = run(&home, &[]);
    assert!(
        !no_root.contains("node_modules"),
        "no --root => no artifact scan"
    );

    // With --root: found and reclaimable.
    let with_root = run(&home, &["--root", home.join("code").to_str().unwrap()]);
    assert!(with_root.contains("node_modules"));
    assert!(reclaimable_for(&with_root, "node_modules"));
    // Still dry-run — must survive.
    assert!(proj.join("node_modules/dep/index.js").exists());

    fs::remove_dir_all(&home).ok();
}

/// Crude JSON probe: is target `id` present AND marked reclaimable? Avoids a
/// JSON dep in the test by checking the object window around the id.
fn reclaimable_for(json: &str, id: &str) -> bool {
    let needle = format!("\"id\": \"{id}\"");
    if let Some(pos) = json.find(&needle) {
        let window = &json[pos..(pos + 400).min(json.len())];
        return window.contains("\"verdict\": \"reclaimable\"");
    }
    false
}
