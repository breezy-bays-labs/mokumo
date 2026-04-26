//! Purity witness — fails compile-time-adjacent (as a regular cargo test)
//! if any file under `crates/kikan/src/control_plane/**/*.rs` imports a
//! transport-layer crate directly.
//!
//! The `kikan::control_plane::*` module is the pure-fn layer for
//! admin-surface business logic (see
//! `ops/decisions/mokumo/adr-control-plane-data-plane-split.md`). It is
//! callable from HTTP, UDS, and in-process CLI paths without re-wrapping.
//! That guarantee holds only if nothing in this module reaches for a
//! transport concern directly.
//!
//! Forbidden imports (exact prefixes matched at start of `use` lines):
//!   - `axum::` / `axum_login::` — HTTP routing, extractors, session
//!   - `tower::` / `tower_sessions::` / `tower_http::` — middleware
//!   - `http::` — the lower-level HTTP types crate
//!   - `mokumo_shop::` — shop vertical (I1 would also block this, but
//!     we guard here as defense-in-depth in case someone moves a helper)
//!
//! If you're tempted to add one of these to a `control_plane/**` file,
//! the answer is almost always: move the transport concern back into the
//! adapter (`crates/kikan/src/platform/*` for HTTP, `kikan-admin-adapter`
//! for UDS) and return the pure value from the control-plane fn.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_PREFIXES: &[&str] = &[
    "axum::",
    "axum_login::",
    "tower::",
    "tower_sessions::",
    "tower_http::",
    "http::",
    "mokumo_shop::",
];

/// Walks `dir` recursively and returns every `.rs` file path it finds.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    out
}

/// Extract the `use …;` target from a line, stripping leading `pub ` and
/// the `use ` keyword. Returns `None` on non-use lines. Handles
/// line-comments by skipping them entirely, but does not attempt to
/// handle block comments (none appear before any `use` in the module
/// today; if one gets added, the test will still correctly flag the
/// forbidden import since `use ...;` lines outside block comments are
/// still scanned).
fn extract_use_target(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return None;
    }
    // Strip `pub ` plus any visibility specifier like `pub(crate) `,
    // `pub(super) `, `pub(in path) `, etc. so re-exports with restricted
    // visibility still get scanned.
    let stripped = if let Some(rest) = trimmed.strip_prefix("pub(") {
        // Skip balanced `(...)` then any whitespace (including none /
        // tabs) so `pub(crate)use ...;` and `pub(super)\tuse ...;` both
        // get scanned.
        rest.find(')')
            .map(|end| rest[end + 1..].trim_start())
            .unwrap_or(trimmed)
    } else {
        trimmed
            .strip_prefix("pub")
            .map(str::trim_start)
            .unwrap_or(trimmed)
    };
    stripped.strip_prefix("use").and_then(|rest| {
        let trimmed = rest.trim_start();
        // Require at least one whitespace char after `use` so identifiers
        // starting with `use` (e.g. `usemod`) don't false-positive.
        (trimmed.len() != rest.len()).then(|| trimmed.trim())
    })
}

/// Whether `use_target` begins with any of the forbidden prefixes.
fn violates(use_target: &str) -> Option<&'static str> {
    FORBIDDEN_PREFIXES
        .iter()
        .copied()
        .find(|prefix| use_target.starts_with(prefix))
}

#[test]
fn control_plane_is_transport_free() {
    let control_plane_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/control_plane");
    assert!(
        control_plane_dir.is_dir(),
        "expected control_plane source directory at {}",
        control_plane_dir.display()
    );

    // Structural witness: the auth submodule is part of the control-plane
    // surface. Asserting it explicitly catches accidental deletion or
    // path drift before the (silently empty) recursion can mask it.
    let auth_dir = control_plane_dir.join("auth");
    assert!(
        auth_dir.is_dir(),
        "expected control_plane/auth source directory at {}",
        auth_dir.display()
    );

    let mut violations: Vec<String> = Vec::new();

    for file in rust_files(&control_plane_dir) {
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));

        for (lineno, line) in content.lines().enumerate() {
            let Some(use_target) = extract_use_target(line) else {
                continue;
            };
            if let Some(prefix) = violates(use_target) {
                violations.push(format!(
                    "{}:{}: forbidden import prefix `{prefix}` — {}",
                    file.display(),
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "control_plane purity violated — the pure-fn layer cannot \
         import transport-layer crates. Offending lines:\n{}",
        violations.join("\n")
    );
}

#[test]
fn violation_detection_catches_canonical_cases() {
    // Self-test: the detector must fire on the patterns it's meant to
    // catch. Keeps the positive case under test so the real check can't
    // silently pass because the detector regressed.
    assert!(violates("axum::extract::State").is_some());
    assert!(violates("tower_sessions::Session").is_some());
    assert!(violates("axum_login::AuthSession").is_some());
    assert!(violates("http::StatusCode").is_some());
    assert!(violates("mokumo_shop::customer::CustomerId").is_some());

    // Negative cases — types owned by kikan itself must not trip.
    assert!(violates("crate::auth::AuthenticatedUser").is_none());
    assert!(violates("sea_orm::DatabaseConnection").is_none());
    assert!(violates("kikan_types::error::ErrorCode").is_none());
    assert!(violates("kikan::error::DomainError").is_none());
}

#[test]
fn extract_use_target_handles_leading_pub_and_whitespace() {
    assert_eq!(
        extract_use_target("use axum::extract::State;"),
        Some("axum::extract::State;")
    );
    assert_eq!(
        extract_use_target("    pub use crate::auth::User;"),
        Some("crate::auth::User;")
    );
    assert_eq!(extract_use_target("// use axum::Router;"), None);
    assert_eq!(extract_use_target("fn foo() {}"), None);
    // Restricted-visibility re-exports are scanned too.
    assert_eq!(
        extract_use_target("pub(crate) use axum::Router;"),
        Some("axum::Router;")
    );
    assert_eq!(
        extract_use_target("    pub(super) use tower::Service;"),
        Some("tower::Service;")
    );
    // Whitespace-tolerant: zero-or-tab whitespace between `pub(...)` and
    // `use` must still get scanned (rustfmt normalizes, but the witness
    // is defense-in-depth).
    assert_eq!(
        extract_use_target("pub(crate)use axum::Router;"),
        Some("axum::Router;")
    );
    assert_eq!(
        extract_use_target("pub(super)\tuse tower::Service;"),
        Some("tower::Service;")
    );
    // `use` keyword must be followed by whitespace — no false positives
    // on identifiers like `useful` or `usemod`.
    assert_eq!(extract_use_target("let useful = 1;"), None);
}
