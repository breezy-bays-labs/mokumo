//! Kikan wire types — `ts-rs`-exported DTOs shared between the Rust
//! server and the SvelteKit SPA.
//!
//! No workspace dependencies; widely consumed by `kikan`,
//! `mokumo-shop`, and the desktop/server binaries. Add a new shared
//! API DTO here, derive `Serialize` + `TS`, then run
//! `moon run shop:gen-types` to regenerate the TypeScript bindings.
//! `DeriveEntityModel` (SeaORM) types must not live here — those are
//! infrastructure and stay with their repo impl (see
//! `ops/decisions/mokumo/adr-entity-type-placement.md`).

pub mod activity;
pub mod admin;
pub mod auth;
pub mod diagnostics;
pub mod error;
pub mod pagination;
pub mod profile;
pub mod settings;
pub mod setup;
pub mod user;
pub mod ws;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use profile::SetupMode;

/// Semver of the HTTP API contract the engine speaks.
///
/// Consumed at build time by the admin SPA (baked into
/// `__KIKAN_ADMIN_UI_BUILT_FOR__`) and at runtime by `/api/kikan-version`.
/// A drift test in `apps/web` pins the admin UI's baked value to this
/// constant — bump both or neither.
pub const API_VERSION: &str = "1.0.0";

/// Typed error payload emitted as a Tauri `"server-error"` event when the server
/// fails to start in the restart loop (after the initial setup phase).
///
/// The `code` tag allows the frontend to branch on the specific failure kind.
/// `backup_path` (when `Some`) points to the pre-migration backup that was taken
/// before the failure — the shop owner can use it to restore their data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ServerStartupError {
    /// A database migration could not be applied.
    MigrationFailed {
        path: String,
        message: String,
        backup_path: Option<String>,
    },
    /// The database was created by a newer version of Mokumo.
    SchemaIncompatible {
        path: String,
        unknown_migrations: Vec<String>,
        backup_path: Option<String>,
    },
    /// The database file is not a Mokumo database (wrong application_id).
    /// Guard 1 fires before Guard 2, so no backup exists at this point.
    NotMokumoDatabase { path: String },
}

/// A single pre-migration backup entry for a database profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupEntry {
    /// Absolute path to the backup file on disk.
    pub path: String,
    /// Migration version string the backup was taken at (e.g. `"m20260404_000000_set_pragmas"`).
    pub version: String,
    /// RFC 3339 timestamp from the backup file's mtime (best-effort).
    pub backed_up_at: String,
}

/// All backup entries for a single database profile (production or demo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProfileBackups {
    /// Newest backup first.
    pub backups: Vec<BackupEntry>,
}

/// Response from `GET /api/backup-status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupStatusResponse {
    pub production: ProfileBackups,
    pub demo: ProfileBackups,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    pub database: String,
    pub install_ok: bool,
    pub storage_ok: bool,
}

/// Response from `GET /api/kikan-version`.
///
/// Returned unauthenticated so the admin SPA can compare its baked-in
/// `api_version` against the live engine before the login page renders.
/// On mismatch, the SPA surfaces a non-blocking banner; on match, no UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KikanVersionResponse {
    /// Semver of the HTTP API contract. The admin SPA bakes this value
    /// at build time and compares to the live response.
    pub api_version: String,
    /// Semver of the running `kikan` crate (from `CARGO_PKG_VERSION`).
    pub engine_version: String,
    /// Short git SHA baked in at build time via `build.rs`. `"unknown"`
    /// when the engine was built outside a git tree.
    pub engine_commit: String,
    /// Applied-migration name per database, keyed by the profile directory
    /// name the graft declared (e.g. `"demo"`, `"production"`). Value is
    /// the highest `seaql_migrations.version` for that pool, or the empty
    /// string when the pool has no migrations applied.
    #[ts(type = "Record<string, string>")]
    pub schema_versions: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServerInfoResponse {
    pub lan_url: Option<String>,
    pub ip_url: Option<String>,
    pub mdns_active: bool,
    pub host: String,
    #[ts(type = "number")]
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        ServerStartupError::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ServerStartupError TypeScript bindings");
        HealthResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export TypeScript bindings");
        KikanVersionResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export KikanVersionResponse TypeScript bindings");
        ServerInfoResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ServerInfoResponse TypeScript bindings");
        BackupEntry::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export BackupEntry TypeScript bindings");
        ProfileBackups::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileBackups TypeScript bindings");
        BackupStatusResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export BackupStatusResponse TypeScript bindings");
        setup::SetupStatusResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export SetupStatusResponse TypeScript bindings");
        setup::DemoResetResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export DemoResetResponse TypeScript bindings");
        setup::ProfileSwitchRequest::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileSwitchRequest TypeScript bindings");
        setup::ProfileSwitchResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileSwitchResponse TypeScript bindings");
        diagnostics::DiagnosticsResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export DiagnosticsResponse TypeScript bindings");
        diagnostics::SystemDiagnostics::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export SystemDiagnostics TypeScript bindings");
        settings::LanAccessResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export LanAccessResponse TypeScript bindings");
        settings::LanAccessRequest::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export LanAccessRequest TypeScript bindings");
        admin::ProfileListResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileListResponse TypeScript bindings");
        admin::ProfileInfo::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileInfo TypeScript bindings");
        admin::ProfileSwitchAdminRequest::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileSwitchAdminRequest TypeScript bindings");
        admin::ProfileSwitchAdminResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileSwitchAdminResponse TypeScript bindings");
        admin::MigrationStatusResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export MigrationStatusResponse TypeScript bindings");
        admin::ProfileMigrationStatus::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ProfileMigrationStatus TypeScript bindings");
        admin::AppliedMigration::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export AppliedMigration TypeScript bindings");
        admin::BackupCreateRequest::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export BackupCreateRequest TypeScript bindings");
        admin::BackupCreatedResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export BackupCreatedResponse TypeScript bindings");
    }

    mod proptest_roundtrips {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn health_response_serialization_roundtrip(
                status in "[a-zA-Z_]{1,20}",
                version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
                uptime_seconds in 0u64..1_000_000,
                database in "[a-zA-Z_]{1,10}",
                install_ok in proptest::bool::ANY,
                storage_ok in proptest::bool::ANY,
            ) {
                let original = HealthResponse { status, version, uptime_seconds, database, install_ok, storage_ok };
                let json = serde_json::to_string(&original).unwrap();
                let restored: HealthResponse = serde_json::from_str(&json).unwrap();
                assert_eq!(original, restored);
            }
        }
    }
}
