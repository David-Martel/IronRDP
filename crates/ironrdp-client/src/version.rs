//! Compile-time version information derived from git tags and build environment.
//!
//! All values are embedded at compile time by `build.rs`.  They reflect the
//! state of the repository at the moment the binary was built.
//!
//! # Examples
//!
//! ```rust
//! use ironrdp_client::version;
//!
//! // Print a human-readable version line.
//! println!("IronRDP {}", version::display_version());
//! ```

/// Full semantic version string (e.g., `"0.1.0-dev.87+cb0506a5"`).
///
/// On a clean tag this is `"<major>.<minor>.<patch>"`.  On a snapshot build
/// between tags the format is `"<ver>-dev.<commits>+<hash>"`.
pub const VERSION: &str = env!("IRONRDP_VERSION");

/// Major version component as a string (e.g., `"0"`).
pub const VERSION_MAJOR: &str = env!("IRONRDP_VERSION_MAJOR");

/// Minor version component as a string (e.g., `"1"`).
pub const VERSION_MINOR: &str = env!("IRONRDP_VERSION_MINOR");

/// Patch version component as a string (e.g., `"0"`).
pub const VERSION_PATCH: &str = env!("IRONRDP_VERSION_PATCH");

/// Short git commit hash at build time (e.g., `"cb0506a5"`).
///
/// Set to `"unknown"` when git is unavailable during the build.
pub const GIT_HASH: &str = env!("IRONRDP_GIT_HASH");

/// Whether the working tree had uncommitted changes at build time.
///
/// Value is `"true"` or `"false"`.
pub const GIT_DIRTY: &str = env!("IRONRDP_GIT_DIRTY");

/// Build timestamp in ISO 8601 UTC format (e.g., `"2026-03-16T14:22:05Z"`).
///
/// Set to `"unknown"` when a timestamp cannot be obtained during the build.
pub const BUILD_TIMESTAMP: &str = env!("IRONRDP_BUILD_TIMESTAMP");

/// Returns a user-facing version string for display and logging.
///
/// Appends `" (dirty)"` when the binary was built from an unclean tree so
/// that diagnostics make it obvious the build may not be reproducible.
///
/// # Examples
///
/// ```rust
/// // Returns something like "0.1.0-dev.87+cb0506a5 (dirty)" or "0.1.0".
/// let _ = ironrdp_client::version::display_version();
/// ```
pub fn display_version() -> String {
    if GIT_DIRTY == "true" {
        format!("{VERSION} (dirty)")
    } else {
        VERSION.to_owned()
    }
}
