use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiagnosticsResponse {
    pub app: AppDiagnostics,
    pub database: DatabaseDiagnostics,
    pub runtime: RuntimeDiagnostics,
    pub os: OsDiagnostics,
    pub system: SystemDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppDiagnostics {
    pub name: String,
    pub version: String,
    /// Git commit SHA at build time; `None` when the binary was not built with vergen.
    pub build_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SystemDiagnostics {
    pub hostname: Option<String>,
    #[ts(type = "number")]
    pub total_memory_bytes: u64,
    #[ts(type = "number")]
    pub used_memory_bytes: u64,
    /// `None` when no disk volume contains the data directory.
    #[ts(type = "number | null")]
    pub disk_total_bytes: Option<u64>,
    /// `None` when no disk volume contains the data directory.
    #[ts(type = "number | null")]
    pub disk_free_bytes: Option<u64>,
    /// True when available disk space is below the warning threshold
    /// (`MOKUMO_DISK_WARNING_THRESHOLD_BYTES`, default 500 MB).
    pub disk_warning: bool,
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
    /// Write-ahead log size in bytes; 0 when no WAL file exists.
    #[ts(type = "number")]
    pub wal_size_bytes: u64,
    /// True when the freelist exceeds 20 % of total pages (VACUUM advisable).
    pub vacuum_needed: bool,
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
