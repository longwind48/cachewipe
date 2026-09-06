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
    // Named app cache rather than the whole of ~/Library/Caches: the blanket
    // target could only ever be all-or-nothing over a directory holding some
    // semi-durable state, so nobody opted in and the tier went unused.
    write_file(
        &home.join("Library/Caches/com.spotify.client/blob.bin"),
        2048,
    );

    let default_json = run(&home, &[]);
    assert!(
        !reclaimable_for(&default_json, "spotify-cache"),
        "os cache must not be reclaimable by default"
    );

    let opt_json = run(&home, &["--include-os-caches"]);
    assert!(
        reclaimable_for(&opt_json, "spotify-cache"),
        "opt-in should surface the named os cache"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn vm_disk_is_sized_and_reported_but_needs_opt_in() {
    let home = scratch("vmdisk");
    // Docker Desktop's disk is a single large FILE, not a directory. The
    // dir-only sizing path reported 0 bytes for it, which hid the biggest
    // reclaimable object on a real machine.
    let raw = home.join("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw");
    write_file(&raw, 200_000);

    let default_json = run(&home, &[]);
    assert!(
        default_json.contains("\"id\": \"docker-desktop-disk\""),
        "VM disks must always be reported — their size is the headline number"
    );
    assert!(
        !reclaimable_for(&default_json, "docker-desktop-disk"),
        "VM disk must not be deletable without --include-vm-disks"
    );
    assert!(
        default_json.contains("needs --include-vm-disks"),
        "the report must say which flag unlocks it"
    );
    // Sized despite being a file, and counted as held-back rather than promised.
    assert!(
        !default_json.contains("\"total_opt_in_bytes\": 0"),
        "a file-backed VM disk must be sized, not reported as 0 bytes"
    );
    assert!(raw.exists(), "reporting must not delete");

    // Opt in and it is removed — remove_file, not remove_dir_all.
    let opt_json = run(&home, &["--apply", "--include-vm-disks"]);
    assert!(reclaimable_for(&opt_json, "docker-desktop-disk"));
    assert!(
        !raw.exists(),
        "--include-vm-disks --apply must remove the file"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn target_dir_needs_a_build_manifest() {
    let home = scratch("targetdir");
    // A plain data directory that happens to be named `target`. Nothing
    // regenerates this, so matching on name alone would be data loss.
    let data = home.join("code/etl");
    write_file(&data.join("target/2026-01-output.parquet"), 4096);
    // A real Rust project, proven by the sibling manifest.
    let rust = home.join("code/tool");
    write_file(&rust.join("Cargo.toml"), 64);
    write_file(&rust.join("target/debug/binary"), 4096);

    let json = run(
        &home,
        &["--apply", "--root", home.join("code").to_str().unwrap()],
    );

    assert!(
        data.join("target/2026-01-output.parquet").exists(),
        "an unproven `target` dir is data, not build output — must survive"
    );
    assert!(
        !rust.join("target").exists(),
        "a manifest-proven target/ is build output and should be reclaimed"
    );
    let _ = json;

    fs::remove_dir_all(&home).ok();
}

#[test]
fn nested_candidates_are_not_double_counted() {
    let home = scratch("nested");
    let app = home.join("code/app");
    // node_modules nested inside .next: both targets match, but the bytes are
    // the same bytes. Counting them twice inflates the headline total.
    write_file(&app.join(".next/standalone/node_modules/dep/i.js"), 100_000);
    write_file(&app.join(".next/build-manifest.json"), 1_000);

    let json = run(&home, &["--root", home.join("code").to_str().unwrap()]);

    // The inner path must not appear as its own reclaimable item.
    assert!(
        !json.contains("standalone/node_modules"),
        "inner nested candidate must be dropped in favour of the outer one: {json}"
    );
    // And the total must equal the outer dir's real size, not ~2x it.
    let total = total_reclaimable(&json);
    assert!(
        total < 150_000,
        "total {total} looks double-counted (outer .next is ~101KB)"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn keeps_newest_version_per_product() {
    let home = scratch("versions");
    let pw = home.join("Library/Caches/ms-playwright");
    // Two products, two builds each. Keep-newest must be per product, or the
    // newest chromium would wipe every firefox.
    for (name, bytes) in [
        ("chromium-1100", 1024),
        ("chromium-1200", 1024),
        ("firefox-1400", 1024),
        ("firefox-1500", 1024),
    ] {
        write_file(&pw.join(name).join("blob.bin"), bytes);
        // Make the higher build number the newer one on disk.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let json = run(&home, &["--apply"]);
    let _ = json;

    assert!(pw.join("chromium-1200").exists(), "newest chromium kept");
    assert!(pw.join("firefox-1500").exists(), "newest firefox kept");
    assert!(
        !pw.join("chromium-1100").exists(),
        "stale chromium reclaimed"
    );
    assert!(!pw.join("firefox-1400").exists(), "stale firefox reclaimed");

    fs::remove_dir_all(&home).ok();
}

#[test]
fn finds_pnpm_store_in_the_macos_location() {
    let home = scratch("pnpm");
    // The store lives under ~/Library on macOS. Checking only the Linux path
    // meant reporting "not present" while gigabytes sat here.
    write_file(&home.join("Library/pnpm/store/v3/files/00/abc"), 8192);

    let json = run(&home, &[]);
    assert!(
        reclaimable_for(&json, "pnpm"),
        "pnpm store must be found in the platform location: {json}"
    );
    // PNPM_HOME itself holds the pnpm binary and global installs — never a target.
    assert!(
        !json.contains(&format!(
            "\"path\": \"{}\"",
            home.join("Library/pnpm").display()
        )),
        "must target the store, never PNPM_HOME itself"
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
///
/// Checks every occurrence, since a target can now resolve to several paths
/// (platform variants, one item per stale version) and only some may qualify.
fn reclaimable_for(json: &str, id: &str) -> bool {
    let needle = format!("\"id\": \"{id}\"");
    let mut from = 0;
    while let Some(rel) = json[from..].find(&needle) {
        let pos = from + rel;
        // Wide enough to cover a long temp $HOME path plus a long `regenerates`
        // string; a tight window silently reads past `verdict` and false-fails.
        let window = &json[pos..(pos + 1200).min(json.len())];
        if window.contains("\"verdict\": \"reclaimable\"") {
            return true;
        }
        from = pos + needle.len();
    }
    false
}

fn total_reclaimable(json: &str) -> u64 {
    let key = "\"total_reclaimable_bytes\": ";
    let pos = json.find(key).expect("total in report") + key.len();
    let tail = &json[pos..];
    let end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    tail[..end].parse().unwrap()
}
