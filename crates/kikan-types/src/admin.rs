//! Admin-only response types for the Unix domain socket control surface.
//!
//! These types are consumed by `kikan-cli` subcommand modules and returned
//! by the admin UDS endpoints in `services/api/src/admin_uds.rs`. They are
//! NOT rendered in the SvelteKit frontend and therefore omit `#[derive(TS)]`.

use serde::{Deserialize, Serialize};

use crate::SetupMode;

// ── Profile listing ──────────────────────────────────────────────────

/// Response from `GET /profiles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileListResponse {
    pub profiles: Vec<ProfileInfo>,
    pub active: SetupMode,
}

/// Status of a single profile (demo or production).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: SetupMode,
    pub active: bool,
    /// `PRAGMA user_version` value, or 0 if the database is not initialized.
    pub schema_version: i64,
    /// Database file size in bytes, or `None` if the file does not exist.
    pub file_size_bytes: Option<u64>,
}

// ── Profile switching (admin) ────────────────────────────────────────

/// Request body for `POST /profiles/switch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSwitchAdminRequest {
    pub profile: SetupMode,
}

/// Response from `POST /profiles/switch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSwitchAdminResponse {
    pub previous: SetupMode,
    pub current: SetupMode,
}

// ── Migration status ─────────────────────────────────────────────────

/// Response from `GET /migrate/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStatusResponse {
    pub production: ProfileMigrationStatus,
    pub demo: ProfileMigrationStatus,
}

/// Migration status for a single profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMigrationStatus {
    pub applied: Vec<AppliedMigration>,
    pub schema_version: i64,
}

/// A single applied migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedMigration {
    pub graft_id: String,
    pub name: String,
    /// Unix timestamp (seconds since epoch) when the migration was applied.
    pub applied_at: i64,
}

// ── Backup operations ────────────────────────────────────────────────

/// Request body for `POST /backups/create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupCreateRequest {
    /// Profile to back up. Defaults to the active profile if omitted.
    pub profile: Option<SetupMode>,
}

/// Response from `POST /backups/create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupCreatedResponse {
    pub path: String,
    pub size: u64,
    pub profile: SetupMode,
}
