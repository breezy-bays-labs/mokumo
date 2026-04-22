//! `GET /api/backup-status` — list pre-migration backup files for both profiles.
//!
//! Unprotected: the shop owner may need to find backup paths even when the
//! server is healthy but before they have authenticated (e.g., immediately
//! after an upgrade). No sensitive data is returned — only file paths on the
//! local machine that Mokumo itself created.

use axum::{Json, extract::State};
use kikan_types::{BackupEntry, BackupStatusResponse, ProfileBackups};

use crate::PlatformState;

pub async fn handler(State(state): State<PlatformState>) -> Json<BackupStatusResponse> {
    // The wire DTO still has `production` + `demo` fields (kikan-types
    // wire shape). Match dir-names to the corresponding wire slot.
    let mut production = ProfileBackups { backups: vec![] };
    let mut demo = ProfileBackups { backups: vec![] };
    for dir in state.profile_dir_names.iter() {
        let path = state.data_dir.join(dir.as_str()).join(state.db_filename);
        let entries = collect_profile_backups(&path).await;
        match dir.as_str() {
            "production" => production = entries,
            "demo" => demo = entries,
            _ => {}
        }
    }
    Json(BackupStatusResponse { production, demo })
}

async fn collect_profile_backups(db_path: &std::path::Path) -> ProfileBackups {
    let backups = match crate::backup::collect_existing_backups(db_path).await {
        Ok(b) => b,
        Err(_) => return ProfileBackups { backups: vec![] },
    };

    // API returns newest-first; collect_existing_backups returns oldest-first.
    let entries: Vec<BackupEntry> = backups
        .into_iter()
        .rev()
        .map(|(path, mtime)| {
            let version = extract_version(path.to_str().unwrap_or(""));
            let backed_up_at = format_mtime(mtime);
            BackupEntry {
                path: path.display().to_string(),
                version,
                backed_up_at,
            }
        })
        .collect();

    ProfileBackups { backups: entries }
}

/// Extract the migration version string from a backup file path.
///
/// Backup files are named `{db}.backup-v{version}`. Returns the part after
/// `.backup-v`, or an empty string if the path does not match.
fn extract_version(path: &str) -> String {
    path.rsplit_once(".backup-v")
        .map(|(_, ver)| ver.to_owned())
        .unwrap_or_default()
}

/// Format a `SystemTime` as an RFC 3339 UTC timestamp string.
///
/// Falls back to Unix epoch string representation on conversion failure.
fn format_mtime(mtime: std::time::SystemTime) -> String {
    use chrono::{DateTime, Utc};
    DateTime::<Utc>::from(mtime).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
