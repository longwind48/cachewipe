//! The catalog of what cachewipe is willing to touch.
//!
//! This module is the trust boundary. Everything cachewipe deletes must be
//! declared here with an explicit regeneration story, so a reader can audit
//! exactly what the tool will ever remove. There is no "delete an arbitrary
//! path" code path anywhere else — deletion only ever operates on paths that
//! a `Target` in this catalog resolved.

use std::path::PathBuf;

/// Risk tier controls default inclusion. We never surprise the user: only
/// fully-regenerable caches are deletable by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Tier {
    /// Package-manager download caches. Deleting these only forces a re-download.
    PackageCache,
    /// Build artifacts inside project dirs. Regenerate from a build command.
    BuildArtifact,
    /// Container engine cache. Delegated to the engine's own prune command.
    Docker,
    /// OS/application caches. Some apps keep semi-durable state here, so this
    /// tier is OFF unless the user opts in with --include-os-caches.
    OsCache,
    /// Container/VM machine disk images. These are the biggest thing on a
    /// developer laptop by a wide margin, and also the heaviest to lose: the
    /// disk holds every local image, container and named volume for its engine.
    /// So we always SIZE and REPORT them — a tool that stays silent about the
    /// largest reclaimable item on the disk is not doing its job — but we never
    /// delete one without --include-vm-disks.
    VmDisk,
}

impl Tier {
    /// Deletable without an explicit opt-in flag?
    pub fn default_on(self) -> bool {
        !matches!(self, Tier::OsCache | Tier::VmDisk)
    }
    /// Sized and shown in the report even when not deletable. VM disks qualify
    /// because their size is the number the user most needs to see.
    pub fn always_reported(self) -> bool {
        matches!(self, Tier::VmDisk)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::PackageCache => "package-cache",
            Tier::BuildArtifact => "build-artifact",
            Tier::Docker => "docker",
            Tier::OsCache => "os-cache",
            Tier::VmDisk => "vm-disk",
        }
    }
}

/// How a target's paths are discovered.
pub enum Kind {
    /// Fixed cache locations under $HOME, tried in order — all that exist are
    /// reported. A list rather than a single path because the same tool stores
    /// its cache somewhere else per platform (pnpm's store is under
    /// ~/.local/share on Linux but ~/Library/pnpm on macOS). Checking only one
    /// meant silently reporting "not present" while gigabytes sat in the other.
    HomeDirs(&'static [&'static str]),
    /// A directory named `name`, found by walking under each root, pruned at
    /// first match so we never descend into (e.g.) node_modules/node_modules.
    ///
    /// `requires_sibling` guards names that are only *sometimes* build output.
    /// `target/` is the clear case: it is Cargo's build dir, but it is also a
    /// perfectly ordinary data directory name (source/ + target/ shows up all
    /// over ETL and ML repos). Matching on name alone would delete real data
    /// that no build command can regenerate, so such a target only matches when
    /// one of these manifest files sits next to it. Empty means match on name.
    NamedDirUnder {
        name: &'static str,
        requires_sibling: &'static [&'static str],
    },
    /// A directory whose children are versioned installs — keep the newest
    /// `keep` of each product, reclaim the rest.
    ///
    /// This exists because all-or-nothing is the wrong operation for these.
    /// Browser-driver caches accumulate one full browser per release: a real
    /// machine had 8 Chrome builds under ~/.cache/puppeteer and 4 Chromiums
    /// under ms-playwright. Deleting the whole directory forces a re-download
    /// of the version actually in use; deleting only the stale ones is both the
    /// bigger win and the safer move.
    ///
    /// Children are grouped by the text before their last `-` so mixed
    /// directories stay correct: `chromium-1187` and `firefox-1490` are
    /// different products and each keeps its own newest. A child with no `-` is
    /// its own group, so it is always kept — an unrecognised layout does
    /// nothing rather than something wrong.
    VersionedDirs {
        subpaths: &'static [&'static str],
        keep: usize,
    },
    /// Handled by shelling out to the tool's own cleanup command. Never a raw
    /// rm: the engine knows what is safe to drop and we do not.
    External {
        probe: &'static str,
        apply_args: &'static [&'static str],
        /// What the delegated command does and does not cover. Surfaced
        /// verbatim, because the gap between the two is exactly where a cleanup
        /// tool loses trust.
        note: &'static str,
    },
}

pub struct Target {
    pub id: &'static str,
    pub tier: Tier,
    /// Plain-English reason it is safe to delete — surfaced to the user.
    pub regenerates: &'static str,
    pub kind: Kind,
}

/// The full catalog. Adding an entry here is the ONLY way to make cachewipe
/// aware of a new deletable thing — keep it auditable.
///
/// Deliberately absent: site-specific and corporate-internal caches. They can
/// be the biggest win on a work laptop (one had 53 stacked copies of a single
/// internal tool manager) but they do not belong in a public catalog. See
/// references/targets.md for how to carry those locally.
pub fn catalog() -> Vec<Target> {
    use Kind::*;
    use Tier::*;
    vec![
        // --- Language / package-manager caches (fully regenerable) ---
        Target {
            id: "uv",
            tier: PackageCache,
            regenerates: "re-downloaded on next `uv sync`/`uv pip install`",
            kind: HomeDirs(&[".cache/uv", "Library/Caches/uv"]),
        },
        Target {
            id: "pip",
            tier: PackageCache,
            regenerates: "re-downloaded on next `pip install`",
            kind: HomeDirs(&[".cache/pip", "Library/Caches/pip"]),
        },
        Target {
            id: "npm",
            tier: PackageCache,
            regenerates: "re-downloaded on next `npm install`",
            kind: HomeDirs(&[".npm/_cacache"]),
        },
        Target {
            id: "npx",
            tier: PackageCache,
            // Separate from the npm cache: _npx holds fully-installed throwaway
            // package trees, and outgrew _cacache 7:1 on a real machine.
            regenerates: "re-installed on next `npx <pkg>` run",
            kind: HomeDirs(&[".npm/_npx"]),
        },
        Target {
            id: "yarn",
            tier: PackageCache,
            regenerates: "re-downloaded on next `yarn install`",
            kind: HomeDirs(&[".cache/yarn", "Library/Caches/Yarn"]),
        },
        Target {
            id: "pnpm",
            tier: PackageCache,
            // Only ever the store subdir. PNPM_HOME (~/Library/pnpm) also holds
            // the pnpm binary and global installs, so the parent is off-limits.
            regenerates: "re-fetched into the pnpm store on next install",
            kind: HomeDirs(&[".local/share/pnpm/store", "Library/pnpm/store"]),
        },
        Target {
            id: "cargo-registry",
            tier: PackageCache,
            regenerates: "re-downloaded on next `cargo build`",
            kind: HomeDirs(&[".cargo/registry/cache"]),
        },
        Target {
            id: "go-mod",
            tier: PackageCache,
            regenerates: "re-downloaded on next `go build`",
            kind: HomeDirs(&["go/pkg/mod/cache/download"]),
        },
        Target {
            id: "gradle",
            tier: PackageCache,
            regenerates: "re-downloaded on next Gradle build",
            kind: HomeDirs(&[".gradle/caches"]),
        },
        Target {
            id: "huggingface",
            tier: PackageCache,
            regenerates: "re-downloaded from the HF hub on next use",
            kind: HomeDirs(&[".cache/huggingface"]),
        },
        // --- Versioned tool installs (keep the newest, drop the rest) ---
        Target {
            id: "puppeteer",
            tier: PackageCache,
            regenerates: "re-downloaded by puppeteer on next run",
            kind: VersionedDirs {
                subpaths: &[
                    ".cache/puppeteer/chrome",
                    ".cache/puppeteer/chrome-headless-shell",
                    ".cache/puppeteer/firefox",
                ],
                keep: 1,
            },
        },
        Target {
            id: "playwright",
            tier: PackageCache,
            regenerates: "re-downloaded by `playwright install`",
            kind: VersionedDirs {
                subpaths: &["Library/Caches/ms-playwright", ".cache/ms-playwright"],
                keep: 1,
            },
        },
        // --- Project build artifacts (regenerate from a build) ---
        // These walk the scan roots; deletion requires the dir be under a root.
        Target {
            id: "node_modules",
            tier: BuildArtifact,
            regenerates: "`npm install` / `pnpm install`",
            kind: NamedDirUnder {
                name: "node_modules",
                requires_sibling: &[],
            },
        },
        Target {
            id: "venv",
            tier: BuildArtifact,
            regenerates: "`uv sync` / `python -m venv`",
            kind: NamedDirUnder {
                name: ".venv",
                requires_sibling: &[],
            },
        },
        Target {
            id: "next",
            tier: BuildArtifact,
            regenerates: "`next build`",
            kind: NamedDirUnder {
                name: ".next",
                requires_sibling: &[],
            },
        },
        Target {
            id: "cargo-target",
            tier: BuildArtifact,
            regenerates: "`cargo build` / `mvn package`",
            kind: NamedDirUnder {
                name: "target",
                requires_sibling: &["Cargo.toml", "pom.xml", "build.gradle", "build.gradle.kts"],
            },
        },
        Target {
            id: "pycache",
            tier: BuildArtifact,
            regenerates: "recompiled by Python on next import",
            kind: NamedDirUnder {
                name: "__pycache__",
                requires_sibling: &[],
            },
        },
        // --- Delegated cleanups (the tool's own command, never a raw rm) ---
        Target {
            id: "docker",
            tier: Docker,
            regenerates: "images/layers rebuilt or re-pulled",
            kind: External {
                probe: "docker",
                apply_args: &["system", "prune", "-f"],
                note: "prunes dangling images + build cache inside the VM; does NOT shrink the VM disk file — see the docker-desktop-disk item for that",
            },
        },
        Target {
            id: "homebrew",
            tier: PackageCache,
            regenerates: "re-downloaded on next `brew install`/`brew upgrade`",
            kind: External {
                probe: "brew",
                apply_args: &["cleanup", "--prune=all"],
                note: "removes stale downloads and old installed versions",
            },
        },
        // --- Container/VM disk images (always reported, opt-in to delete) ---
        Target {
            id: "docker-desktop-disk",
            tier: VmDisk,
            regenerates: "Docker Desktop recreates an empty disk on next launch; ALL local images, containers and named volumes are lost",
            kind: HomeDirs(&["Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw"]),
        },
        Target {
            id: "finch-disk",
            tier: VmDisk,
            regenerates: "`finch vm init` recreates it; all images in the VM are lost",
            kind: HomeDirs(&[".finch/.disks"]),
        },
        Target {
            id: "lima-disk",
            tier: VmDisk,
            regenerates: "`limactl start` recreates the machine; its images are lost",
            kind: HomeDirs(&[".lima"]),
        },
        Target {
            id: "colima-disk",
            tier: VmDisk,
            regenerates: "`colima start` recreates the machine; its images are lost",
            kind: HomeDirs(&[".colima"]),
        },
        Target {
            id: "podman-disk",
            tier: VmDisk,
            regenerates: "`podman machine init` recreates it; its images are lost",
            kind: HomeDirs(&[".local/share/containers/podman"]),
        },
        // --- OS / app caches (opt-in only) ---
        // Named individually rather than as one blanket ~/Library/Caches entry.
        // The blanket version could only ever be all-or-nothing over a directory
        // where some apps keep semi-durable state, so nobody could reasonably
        // opt in and the whole tier went unused. Naming the big, plainly
        // regenerable ones makes the opt-in a decision someone can actually make.
        Target {
            id: "spotify-cache",
            tier: OsCache,
            regenerates: "re-streamed/re-cached by Spotify on next play",
            kind: HomeDirs(&["Library/Caches/com.spotify.client"]),
        },
        Target {
            id: "firefox-cache",
            tier: OsCache,
            regenerates: "refetched by Firefox as you browse",
            kind: HomeDirs(&["Library/Caches/Firefox"]),
        },
        Target {
            id: "chrome-cache",
            tier: OsCache,
            regenerates: "refetched by Chrome as you browse",
            kind: HomeDirs(&["Library/Caches/Google"]),
        },
        Target {
            id: "jsii-cache",
            tier: OsCache,
            regenerates: "rebuilt by the AWS CDK toolchain on next synth",
            kind: HomeDirs(&["Library/Caches/com.amazonaws.jsii"]),
        },
    ]
}

/// Resolve a HomeDir target to an absolute path, given $HOME.
pub fn home_path(home: &str, subpath: &str) -> PathBuf {
    let mut p = PathBuf::from(home);
    p.push(subpath);
    p
}

/// Group key for a versioned install directory: everything before the last `-`.
/// `chromium-1187` -> `chromium`, `mac_arm-121.0.6167.85` -> `mac_arm`.
/// A name with no `-` is its own group, so unfamiliar layouts keep everything.
pub fn version_group(name: &str) -> &str {
    match name.rfind('-') {
        Some(i) => &name[..i],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_versioned_names_by_product() {
        assert_eq!(version_group("chromium-1187"), "chromium");
        assert_eq!(
            version_group("chromium_headless_shell-1228"),
            "chromium_headless_shell"
        );
        assert_eq!(version_group("mac_arm-121.0.6167.85"), "mac_arm");
        assert_eq!(version_group("firefox-1490"), "firefox");
    }

    #[test]
    fn undashed_name_is_its_own_group() {
        // An unrecognised layout must not collapse into one group and lose files.
        assert_eq!(version_group("chrome"), "chrome");
        assert_eq!(version_group(""), "");
    }

    #[test]
    fn target_dir_requires_a_build_manifest() {
        let cat = catalog();
        let t = cat.iter().find(|t| t.id == "cargo-target").unwrap();
        match t.kind {
            Kind::NamedDirUnder {
                requires_sibling, ..
            } => assert!(
                requires_sibling.contains(&"Cargo.toml"),
                "target/ must be manifest-gated or it eats plain data dirs"
            ),
            _ => panic!("cargo-target should be NamedDirUnder"),
        }
    }

    #[test]
    fn vm_disks_are_reported_but_not_default_deletable() {
        assert!(Tier::VmDisk.always_reported());
        assert!(!Tier::VmDisk.default_on());
        assert!(Tier::PackageCache.default_on());
        assert!(!Tier::OsCache.default_on());
    }

    #[test]
    fn pnpm_never_targets_pnpm_home_itself() {
        let cat = catalog();
        let t = cat.iter().find(|t| t.id == "pnpm").unwrap();
        match t.kind {
            Kind::HomeDirs(paths) => {
                for p in paths {
                    assert!(
                        p.ends_with("store"),
                        "PNPM_HOME holds the pnpm binary and global installs; only the store is a cache"
                    );
                }
            }
            _ => panic!("pnpm should be HomeDirs"),
        }
    }
}
