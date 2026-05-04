//! `GET /api/backup-status` — list pre-migration backup files for both
//! Mokumo profiles (production + demo).
//!
//! Shared between the public HTTP surface (mounted from
//! [`crate::routes::data_plane_routes`]) and the UDS admin router
//! ([`crate::admin::router`]). The wire DTO `BackupStatusResponse` names
//! the two `SetupMode` variants directly — kikan-types carries the shape
//! so the SPA can consume it, but the dir-name → DTO-slot dispatch is a
//! Mokumo concern and lives here.
//!
//! Unauthenticated: the shop owner may need to find backup paths even
//! when the server is healthy but before they have authenticated (e.g.
//! immediately after an upgrade). No sensitive data is returned — only
//! file paths on the local machine that Mokumo itself created.

use axum::{Json, extract::State};
use kikan::PlatformState;
use kikan_types::{BackupEntry, BackupStatusResponse, ProfileBackups};

/// Axum handler returning the current backup inventory.
pub async fn handler(State(state): State<PlatformState>) -> Json<BackupStatusResponse> {
    Json(collect(&state).await)
}

/// Transport-neutral collection function, reused by both the HTTP handler
/// and the UDS `backups_list` endpoint.
///
/// Iterates every profile dir-name the Graft declared and maps each to
/// its `BackupStatusResponse` slot. A dir that isn't `"production"` or
/// `"demo"` is logged and dropped — the current wire DTO only carries
/// those two slots. When a second vertical with different profile names
/// lands, the DTO needs to become per-kind (a map) — this function is
/// the place to change.
pub async fn collect(state: &PlatformState) -> BackupStatusResponse {
    let mut production = ProfileBackups { backups: vec![] };
    let mut demo = ProfileBackups { backups: vec![] };

    for dir in state.profile_dir_names.iter() {
        let path = state.data_dir.join(dir.as_str()).join(state.db_filename);
        let entries = collect_profile_backups(&path).await;
        match dir.as_str() {
            "production" => production = entries,
            "demo" => demo = entries,
            other => tracing::debug!(
                dir = other,
                "BackupStatusResponse has no wire slot for this profile dir; dropping",
            ),
        }
    }

    BackupStatusResponse { production, demo }
}

async fn collect_profile_backups(db_path: &std::path::Path) -> ProfileBackups {
    let Ok(backups) = kikan::backup::collect_existing_backups(db_path).await else {
        return ProfileBackups { backups: vec![] };
    };

    let entries: Vec<BackupEntry> = backups
        .into_iter()
        .rev()
        .map(|(path, mtime)| BackupEntry {
            path: path.display().to_string(),
            version: extract_version(path.to_str().unwrap_or("")),
            backed_up_at: format_mtime(mtime),
        })
        .collect();

    ProfileBackups { backups: entries }
}

fn extract_version(path: &str) -> String {
    path.rsplit_once(".backup-v")
        .map(|(_, ver)| ver.to_owned())
        .unwrap_or_default()
}

fn format_mtime(mtime: std::time::SystemTime) -> String {
    use chrono::{DateTime, Utc};
    DateTime::<Utc>::from(mtime).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
