pub mod activity;
pub mod auth;
pub mod customer;
pub mod error;
pub mod pagination;
pub mod setup;
pub mod user;
pub mod ws;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use mokumo_core::setup::SetupMode;

/// Typed error payload emitted as a Tauri `"server-error"` event when the server
/// fails to start in the restart loop (after the initial setup phase).
///
/// The `code` tag allows the frontend to branch on the specific failure kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ServerStartupError {
    /// A database migration could not be applied.
    MigrationFailed { path: String, message: String },
    /// The database was created by a newer version of Mokumo.
    SchemaIncompatible {
        path: String,
        unknown_migrations: Vec<String>,
    },
    /// The database file is not a Mokumo database (wrong application_id).
    NotMokumoDatabase { path: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    pub database: String,
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
        ServerStartupError::export_all()
            .expect("Failed to export ServerStartupError TypeScript bindings");
        HealthResponse::export_all().expect("Failed to export TypeScript bindings");
        ServerInfoResponse::export_all()
            .expect("Failed to export ServerInfoResponse TypeScript bindings");
        setup::SetupStatusResponse::export_all()
            .expect("Failed to export SetupStatusResponse TypeScript bindings");
        setup::DemoResetResponse::export_all()
            .expect("Failed to export DemoResetResponse TypeScript bindings");
        setup::ProfileSwitchRequest::export_all()
            .expect("Failed to export ProfileSwitchRequest TypeScript bindings");
        setup::ProfileSwitchResponse::export_all()
            .expect("Failed to export ProfileSwitchResponse TypeScript bindings");
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
            ) {
                let original = HealthResponse { status, version, uptime_seconds, database };
                let json = serde_json::to_string(&original).unwrap();
                let restored: HealthResponse = serde_json::from_str(&json).unwrap();
                assert_eq!(original, restored);
            }
        }
    }
}
