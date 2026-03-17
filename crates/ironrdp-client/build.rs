//! Build script for ironrdp-client.
//!
//! Embeds git-derived version information as compile-time environment variables so
//! that `src/version.rs` can expose them as `const` values without runtime overhead.
//!
//! Variable resolution order for `IRONRDP_VERSION`:
//!   1. `IRONRDP_BUILD_VERSION` env var (set by release tooling / build.ps1)
//!   2. `git describe --tags --long --dirty --match "v*"` output, parsed into semver
//!   3. `CARGO_PKG_VERSION` with hash "unknown" (offline / no-tag fallback)

use std::process::Command;

fn main() {
    // Re-run when git state changes so version stays accurate on each commit.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    // Re-run if the caller overrides the version via an env var.
    println!("cargo:rerun-if-env-changed=IRONRDP_BUILD_VERSION");

    let timestamp = build_timestamp();

    // If build tooling already computed a canonical version string, honour it.
    if let Ok(override_ver) = std::env::var("IRONRDP_BUILD_VERSION")
        && !override_ver.is_empty()
    {
        // The override must already be a valid semver string; emit it as-is.
        // Extract components from it for the individual variables.
        let (major, minor, patch, hash, dirty) = parse_override_version(&override_ver);
        emit_version_vars(&override_ver, &major, &minor, &patch, &hash, &dirty, &timestamp);
        return;
    }

    // Attempt to derive version from `git describe`.
    match git_describe() {
        Some(described) => {
            let (version, major, minor, patch, hash, dirty) = parse_git_describe(&described);
            emit_version_vars(&version, &major, &minor, &patch, &hash, &dirty, &timestamp);
        }
        None => {
            // Offline or no matching tag — fall back to Cargo metadata.
            let cargo_ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
            let (major, minor, patch) = split_semver(&cargo_ver);
            let version = format!("{cargo_ver}+unknown");
            emit_version_vars(&version, &major, &minor, &patch, "unknown", "false", &timestamp);
        }
    }
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

/// Runs `git describe --tags --long --dirty --match "v*"` and returns its stdout
/// on success, or `None` if the command fails or produces no usable output.
fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--long", "--dirty", "--match", "v*"])
        .output()
        .ok()?;

    if output.status.success() {
        let s = String::from_utf8(output.stdout).ok()?;
        let trimmed = s.trim().to_owned();
        if trimmed.is_empty() { None } else { Some(trimmed) }
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Splits a bare "major.minor.patch" string, returning each component as a
/// `String`.  Missing components default to `"0"`.
fn split_semver(ver: &str) -> (String, String, String) {
    let mut parts = ver.splitn(3, '.');
    let major = parts.next().unwrap_or("0").to_owned();
    let minor = parts.next().unwrap_or("0").to_owned();
    // Patch may carry pre-release / build metadata — strip them.
    let patch_raw = parts.next().unwrap_or("0");
    let patch = patch_raw
        .split(['-', '+'])
        .next()
        .unwrap_or("0")
        .to_owned();
    (major, minor, patch)
}

/// Parses the output of `git describe --tags --long --dirty --match "v*"`.
///
/// Expected format: `v<semver>-<commits>-g<hash>[-dirty]`
/// Example:        `v0.1.0-87-gcb0506a5-dirty`
///
/// Returns `(full_version, major, minor, patch, short_hash, dirty)`.
fn parse_git_describe(described: &str) -> (String, String, String, String, String, String) {
    // Strip the leading "v".
    let without_v = described.strip_prefix('v').unwrap_or(described);

    let dirty = described.ends_with("-dirty");
    let dirty_str = if dirty { "true" } else { "false" };

    // Work on the string without the trailing "-dirty" suffix.
    let base = if dirty {
        without_v
            .strip_suffix("-dirty")
            .unwrap_or(without_v)
    } else {
        without_v
    };

    // Split on '-' from the right to extract hash and commit count.
    // Format after stripping dirty: `<semver>-<commits>-g<hash>`
    let mut rev_parts = base.rsplitn(3, '-');
    let hash_part = rev_parts.next().unwrap_or("unknown");
    let commits_part = rev_parts.next().unwrap_or("0");
    let semver_part = rev_parts.next().unwrap_or("0.0.0");

    // Short hash: strip the leading "g" that git describe adds.
    let short_hash = hash_part.strip_prefix('g').unwrap_or(hash_part);

    let commits: u64 = commits_part.parse().unwrap_or(0);

    let (major, minor, patch) = split_semver(semver_part);

    // Build the full semver string.
    // When there are commits since the tag this is a pre-release snapshot.
    let full_version = if commits == 0 && !dirty {
        // Exactly on a tag, clean tree.
        format!("{major}.{minor}.{patch}")
    } else {
        // Pre-release snapshot: `<ver>-dev.<commits>+<hash>`.
        format!("{major}.{minor}.{patch}-dev.{commits}+{short_hash}")
    };

    (
        full_version,
        major,
        minor,
        patch,
        short_hash.to_owned(),
        dirty_str.to_owned(),
    )
}

/// Parses a pre-computed version override string produced by build tooling.
///
/// The override is expected to already be a well-formed semver string such as
/// `"1.2.3"` or `"1.2.3-dev.87+cb0506a5"`.  We extract major/minor/patch from
/// the numeric prefix and leave hash/dirty at sensible defaults.
fn parse_override_version(ver: &str) -> (String, String, String, String, String) {
    // Extract hash from build metadata if present (after '+').
    let (base, hash) = if let Some((base, hash)) = ver.split_once('+') {
        (base, hash.to_owned())
    } else {
        (ver, "unknown".to_owned())
    };

    // Determine dirty from a trailing "(dirty)" or "-dirty" in the override.
    let dirty = if ver.contains("dirty") { "true" } else { "false" };

    let (major, minor, patch) = split_semver(base);
    (major, minor, patch, hash, dirty.to_owned())
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

/// Emits all `cargo:rustc-env=` directives consumed by `src/version.rs`.
fn emit_version_vars(
    version: &str,
    major: &str,
    minor: &str,
    patch: &str,
    hash: &str,
    dirty: &str,
    timestamp: &str,
) {
    println!("cargo:rustc-env=IRONRDP_VERSION={version}");
    println!("cargo:rustc-env=IRONRDP_VERSION_MAJOR={major}");
    println!("cargo:rustc-env=IRONRDP_VERSION_MINOR={minor}");
    println!("cargo:rustc-env=IRONRDP_VERSION_PATCH={patch}");
    println!("cargo:rustc-env=IRONRDP_GIT_HASH={hash}");
    println!("cargo:rustc-env=IRONRDP_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=IRONRDP_BUILD_TIMESTAMP={timestamp}");
}

/// Produces an ISO 8601 UTC timestamp string.
///
/// We avoid pulling in `time` or `chrono` to keep `build.rs` dependency-free.
/// On systems where `date` or PowerShell is unavailable we fall back to a
/// static placeholder rather than failing the build.
fn build_timestamp() -> String {
    // Try Unix `date` first (Linux / macOS CI).
    if let Ok(out) = Command::new("date").arg("-u").arg("+%Y-%m-%dT%H:%M:%SZ").output()
        && out.status.success()
        && let Ok(s) = String::from_utf8(out.stdout)
    {
        let ts = s.trim().to_owned();
        if !ts.is_empty() {
            return ts;
        }
    }

    // On Windows fall back to PowerShell.
    if let Ok(out) = Command::new("powershell")
        .args(["-NoLogo", "-NoProfile", "-Command", "[datetime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')"])
        .output()
        && out.status.success()
        && let Ok(s) = String::from_utf8(out.stdout)
    {
        let ts = s.trim().to_owned();
        if !ts.is_empty() {
            return ts;
        }
    }

    "unknown".to_owned()
}
