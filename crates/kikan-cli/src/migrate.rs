//! Migration status display via the admin UDS.

use crate::{CliError, UdsClient};
use kikan_types::admin::{MigrationStatusResponse, ProfileMigrationStatus};

/// Fetch and display migration status from the daemon.
pub async fn status(client: &UdsClient, json: bool) -> Result<(), CliError> {
    let body = client.get("/migrate/status").await?;
    let resp: MigrationStatusResponse = serde_json::from_slice(&body)
        .map_err(|e| CliError::Other(format!("invalid migration status response: {e}")))?;

    if json {
        crate::format::print_json(&resp)?;
    } else {
        print_migration_status(&resp);
    }

    Ok(())
}

fn print_migration_status(resp: &MigrationStatusResponse) {
    print_profile_migrations("production", &resp.production);
    println!();
    print_profile_migrations("demo", &resp.demo);
}

fn print_profile_migrations(label: &str, status: &ProfileMigrationStatus) {
    println!(
        "Migrations ({label}) \u{2014} {} applied, schema v{}",
        status.applied.len(),
        status.schema_version
    );
    if status.applied.is_empty() {
        println!("  (none)");
        return;
    }
    println!("  {:<20} {:<50} Applied", "Graft", "Migration");
    println!("  {}", crate::format::separator(80));
    for m in &status.applied {
        let ts = format_timestamp(m.applied_at);
        println!("  {:<20} {:<50} {}", m.graft_id, m.name, ts);
    }
}

fn format_timestamp(unix_secs: i64) -> String {
    // Best-effort human-readable date. Fall back to raw timestamp.
    let secs = unix_secs as u64;
    // Simple UTC date from epoch: days since 1970-01-01.
    let days = secs / 86400;
    // Approximate year/month/day using a simple algorithm.
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil days algorithm (Howard Hinnant).
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
