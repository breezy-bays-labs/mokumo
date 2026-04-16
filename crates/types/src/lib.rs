pub mod activity;
pub mod auth;
pub mod customer;
pub mod diagnostics;
pub mod error;
pub mod pagination;
pub mod setup;
pub mod user;
pub mod ws;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use kikan::SetupMode;

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
