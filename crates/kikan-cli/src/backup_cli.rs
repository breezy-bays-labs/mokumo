//! Backup listing and creation via the admin UDS.

use crate::{CliError, UdsClient};
use kikan_types::BackupStatusResponse;
use kikan_types::admin::{BackupCreateRequest, BackupCreatedResponse};

/// Fetch and display the backup list from the daemon.
pub async fn list(client: &UdsClient, json: bool) -> Result<(), CliError> {
    let body = client.get("/backups").await?;
    let resp: BackupStatusResponse = serde_json::from_slice(&body)
        .map_err(|e| CliError::Other(format!("invalid backups response: {e}")))?;

    if json {
        crate::format::print_json(&resp)?;
    } else {
        print_backup_list(&resp);
    }

    Ok(())
}

/// Request the daemon to create a backup.
pub async fn create(client: &UdsClient, profile: Option<&str>, json: bool) -> Result<(), CliError> {
    let mode = match profile {
        Some(s) => Some(
            s.parse::<kikan_types::SetupMode>()
                .map_err(|e: String| CliError::Other(e))?,
        ),
        None => None,
    };

    let req = BackupCreateRequest { profile: mode };
    let body = client.post("/backups/create", &req).await?;
    let resp: BackupCreatedResponse = serde_json::from_slice(&body)
        .map_err(|e| CliError::Other(format!("invalid backup create response: {e}")))?;

    if json {
        crate::format::print_json(&resp)?;
    } else {
        println!("Backup created: {}", resp.path);
        println!("  Profile: {}", resp.profile);
        println!("  Size:    {} bytes", resp.size);
    }

    Ok(())
}

fn print_backup_list(resp: &BackupStatusResponse) {
    print_profile_backups("production", &resp.production);
    println!();
    print_profile_backups("demo", &resp.demo);
}

fn print_profile_backups(label: &str, backups: &kikan_types::ProfileBackups) {
    println!("Backups ({label}) \u{2014} {} found", backups.backups.len());
    if backups.backups.is_empty() {
        println!("  (none)");
        return;
    }
    println!("  {:<50} {:<12} Path", "Version", "Date");
    println!("  {}", crate::format::separator(90));
    for b in &backups.backups {
        println!("  {:<50} {:<12} {}", b.version, b.backed_up_at, b.path);
    }
}
