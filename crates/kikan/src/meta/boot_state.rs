//! Boot-state detection — figures out what kind of data directory we're
//! looking at so the engine can decide whether to do a fresh install,
//! a normal boot, a legacy upgrade, or refuse with a defensive error.
//!
//! Each variant carries the data its corresponding handler needs (e.g.
//! `LegacyCompleted` carries the display name derived from the legacy
//! vertical DB so the upgrade handler doesn't have to re-open it).

use std::path::PathBuf;

use thiserror::Error;

/// Result of inspecting `<data_dir>` to decide the boot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootState {
    /// No `meta.db`, no profile folders. New install — run the setup wizard.
    FreshInstall,

    /// `meta.db` exists with at least one row in `meta.profiles`. Normal
    /// boot path — open per-profile pools and serve traffic.
    PostUpgradeOrSetup,

    /// `meta.db` is absent, the legacy `production/` folder exists, and the
    /// vertical DB has at least one admin user plus a non-empty display
    /// name. Eligible for the silent legacy upgrade.
    LegacyCompleted {
        vertical_db_path: PathBuf,
        derived_display_name: String,
    },

    /// Legacy `production/` folder exists but the vertical DB has no admin
    /// user and `shop_settings` is empty. Setup wizard was abandoned. Leave
    /// the folder alone and treat the boot as a fresh install.
    LegacyAbandoned { production_dir: PathBuf },

    /// Legacy `production/` folder exists with admin user(s) present BUT
    /// `shop_settings.shop_name` is empty. Refuse to boot — the operator
    /// must repair manually. (Defensive case; protects against
    /// auto-deriving an empty slug.)
    LegacyDefensiveEmpty { production_dir: PathBuf },
}

/// I/O / inspection errors surfaced by [`detect_boot_state`].
#[derive(Debug, Error)]
pub enum BootStateDetectionError {
    #[error("data directory I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not open vertical DB at {path:?}: {source}")]
    OpenVerticalDb {
        path: PathBuf,
        #[source]
        source: sea_orm::DbErr,
    },

    #[error("could not query vertical DB at {path:?}: {source}")]
    QueryVerticalDb {
        path: PathBuf,
        #[source]
        source: sea_orm::DbErr,
    },
}

/// Inspect `<data_dir>` and return the boot state.
///
/// Returns [`BootStateDetectionError::Io`] if no implementation is available
/// yet — callers must propagate, not panic on it.
pub async fn detect_boot_state(
    data_dir: &std::path::Path,
) -> Result<BootState, BootStateDetectionError> {
    Err(BootStateDetectionError::Io {
        path: data_dir.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "detect_boot_state not yet implemented",
        ),
    })
}
