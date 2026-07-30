//! The catalog of what cachewipe is willing to touch.
//!
//! This module is the trust boundary. Everything cachewipe deletes must be
//! declared here with an explicit regeneration story, so a reader can audit
//! exactly what the tool will ever remove. There is no "delete an arbitrary
//! path" code path anywhere else — deletion only ever operates on paths that
//! a `Target` in this catalog resolved.

use std::path::PathBuf;

/// Risk tier controls default inclusion. We never surprise the user: only
/// fully-regenerable caches are on by default.
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
}

impl Tier {
    /// On by default? OS caches are the only opt-in tier.
    pub fn default_on(self) -> bool {
        !matches!(self, Tier::OsCache)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::PackageCache => "package-cache",
            Tier::BuildArtifact => "build-artifact",
            Tier::Docker => "docker",
            Tier::OsCache => "os-cache",
        }
    }
}

/// How a target's paths are discovered.
pub enum Kind {
    /// A fixed cache directory under $HOME (glob-free, exact subpath).
    HomeDir(&'static str),
    /// A directory named `name`, found by walking under each root, pruned at
    /// first match so we never descend into (e.g.) node_modules/node_modules.
    NamedDirUnder { name: &'static str },
    /// Handled by shelling out to an external engine (Docker). Never a raw rm.
    External { probe: &'static str },
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
pub fn catalog() -> Vec<Target> {
    use Kind::*;
    use Tier::*;
    vec![
        // --- Language / package-manager caches (fully regenerable) ---
        Target {
            id: "uv",
            tier: PackageCache,
            regenerates: "re-downloaded on next `uv sync`/`uv pip install`",
            kind: HomeDir(".cache/uv"),
        },
        Target {
            id: "pip",
            tier: PackageCache,
            regenerates: "re-downloaded on next `pip install`",
            kind: HomeDir(".cache/pip"),
        },
        Target {
            id: "npm",
            tier: PackageCache,
            regenerates: "re-downloaded on next `npm install`",
            kind: HomeDir(".npm/_cacache"),
        },
        Target {
            id: "yarn",
            tier: PackageCache,
            regenerates: "re-downloaded on next `yarn install`",
            kind: HomeDir(".cache/yarn"),
        },
        Target {
            id: "pnpm",
            tier: PackageCache,
            regenerates: "re-fetched into the pnpm store on next install",
            kind: HomeDir(".local/share/pnpm/store"),
        },
        Target {
            id: "cargo-registry",
            tier: PackageCache,
            regenerates: "re-downloaded on next `cargo build`",
            kind: HomeDir(".cargo/registry/cache"),
        },
        Target {
            id: "go-mod",
            tier: PackageCache,
            regenerates: "re-downloaded on next `go build`",
            kind: HomeDir("go/pkg/mod/cache/download"),
        },
        Target {
            id: "gradle",
            tier: PackageCache,
            regenerates: "re-downloaded on next Gradle build",
            kind: HomeDir(".gradle/caches"),
        },
        Target {
            id: "huggingface",
            tier: PackageCache,
            regenerates: "re-downloaded from the HF hub on next use",
            kind: HomeDir(".cache/huggingface"),
        },
        // --- Project build artifacts (regenerate from a build) ---
        // These walk the scan roots; deletion requires the dir be under a root.
        Target {
            id: "node_modules",
            tier: BuildArtifact,
            regenerates: "`npm install` / `pnpm install`",
            kind: NamedDirUnder {
                name: "node_modules",
            },
        },
        Target {
            id: "venv",
            tier: BuildArtifact,
            regenerates: "`uv sync` / `python -m venv`",
            kind: NamedDirUnder { name: ".venv" },
        },
        Target {
            id: "next",
            tier: BuildArtifact,
            regenerates: "`next build`",
            kind: NamedDirUnder { name: ".next" },
        },
        Target {
            id: "cargo-target",
            tier: BuildArtifact,
            regenerates: "`cargo build`",
            kind: NamedDirUnder { name: "target" },
        },
        Target {
            id: "pycache",
            tier: BuildArtifact,
            regenerates: "recompiled by Python on next import",
            kind: NamedDirUnder {
                name: "__pycache__",
            },
        },
        // --- Docker (delegated to the engine, never raw rm) ---
        Target {
            id: "docker",
            tier: Docker,
            regenerates: "images/layers rebuilt or re-pulled",
            kind: External { probe: "docker" },
        },
        // --- OS / app caches (opt-in only) ---
        Target {
            id: "os-user-cache",
            tier: OsCache,
            regenerates: "apps rebuild their caches; SOME state may not be regenerable",
            kind: HomeDir("Library/Caches"),
        },
    ]
}

/// Resolve a HomeDir target to an absolute path, given $HOME.
pub fn home_path(home: &str, subpath: &str) -> PathBuf {
    let mut p = PathBuf::from(home);
    p.push(subpath);
    p
}
