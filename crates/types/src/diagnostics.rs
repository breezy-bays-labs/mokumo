use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiagnosticsResponse {
    pub app: AppDiagnostics,
    pub database: DatabaseDiagnostics,
    pub runtime: RuntimeDiagnostics,
    pub os: OsDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppDiagnostics {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagnostics {
    pub production: ProfileDbDiagnostics,
    pub demo: ProfileDbDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProfileDbDiagnostics {
    #[ts(type = "number")]
    pub schema_version: i64,
    #[ts(type = "number | null")]
    pub file_size_bytes: Option<u64>,
    pub wal_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeDiagnostics {
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    #[ts(type = "\"demo\" | \"production\"")]
    pub active_profile: mokumo_core::setup::SetupMode,
    pub setup_complete: bool,
    pub is_first_launch: bool,
    pub mdns_active: bool,
    pub lan_url: Option<String>,
    pub host: String,
    #[ts(type = "number")]
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OsDiagnostics {
    pub family: String,
    pub arch: String,
}
