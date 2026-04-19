//! `diagnose` subcommand — fetch diagnostics from the running daemon via UDS.

use crate::{CliError, UdsClient};

/// Fetch diagnostics from the running daemon and print to stdout.
///
/// When `json` is true, prints the raw JSON. Otherwise prints a
/// human-readable summary.
pub async fn run(client: &UdsClient, json: bool) -> Result<(), CliError> {
    let body = client.get("/diagnostics").await?;

    if json {
        // Pretty-print the JSON.
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| CliError::Other(format!("invalid JSON from daemon: {e}")))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&value)
                .map_err(|e| CliError::Other(format!("JSON format error: {e}")))?
        );
    } else {
        // Parse and print human-readable summary.
        let diag: kikan_types::diagnostics::DiagnosticsResponse = serde_json::from_slice(&body)
            .map_err(|e| CliError::Other(format!("invalid diagnostics response: {e}")))?;
        print_human_readable(&diag);
    }

    Ok(())
}

fn print_human_readable(diag: &kikan_types::diagnostics::DiagnosticsResponse) {
    println!(
        "{} v{} ({})",
        diag.app.name,
        diag.app.version,
        diag.app.build_commit.as_deref().unwrap_or("unknown commit")
    );
    println!();

    println!("Runtime");
    println!("  uptime:        {}s", diag.runtime.uptime_seconds);
    println!("  profile:       {:?}", diag.runtime.active_profile);
    println!(
        "  setup:         {}",
        if diag.runtime.setup_complete {
            "complete"
        } else {
            "pending"
        }
    );
    println!("  first launch:  {}", diag.runtime.is_first_launch);
    println!(
        "  mDNS:          {}",
        if diag.runtime.mdns_active {
            "active"
        } else {
            "inactive"
        }
    );
    if let Some(url) = &diag.runtime.lan_url {
        println!("  LAN URL:       {url}");
    }
    println!();

    println!("Database (production)");
    print_profile_db(&diag.database.production);
    println!("Database (demo)");
    print_profile_db(&diag.database.demo);

    println!("System");
    if let Some(host) = &diag.system.hostname {
        println!("  hostname:      {host}");
    }
    println!("  OS:            {} ({})", diag.os.family, diag.os.arch);
    println!(
        "  memory:        {} / {} MB",
        diag.system.used_memory_bytes / 1_048_576,
        diag.system.total_memory_bytes / 1_048_576
    );
    if let (Some(total), Some(free)) = (diag.system.disk_total_bytes, diag.system.disk_free_bytes) {
        println!(
            "  disk:          {} / {} MB free{}",
            free / 1_048_576,
            total / 1_048_576,
            if diag.system.disk_warning {
                " ⚠ LOW"
            } else {
                ""
            }
        );
    }
}

fn print_profile_db(db: &kikan_types::diagnostics::ProfileDbDiagnostics) {
    println!("  schema:        v{}", db.schema_version);
    if let Some(size) = db.file_size_bytes {
        println!("  size:          {} KB", size / 1024);
    }
    println!(
        "  WAL:           {} ({})",
        if db.wal_mode { "enabled" } else { "disabled" },
        if db.wal_size_bytes > 0 {
            format!("{} KB pending", db.wal_size_bytes / 1024)
        } else {
            "clean".to_string()
        }
    );
    if db.vacuum_needed {
        println!("  vacuum:        needed");
    }
    println!();
}
