//! Profile listing and switching via the admin UDS.

use crate::{CliError, UdsClient};
use kikan_types::admin::{
    ProfileListResponse, ProfileSwitchAdminRequest, ProfileSwitchAdminResponse,
};

/// Fetch and display the profile list from the daemon.
pub async fn list(client: &UdsClient, json: bool) -> Result<(), CliError> {
    let body = client.get("/profiles").await?;
    let resp: ProfileListResponse = serde_json::from_slice(&body)
        .map_err(|e| CliError::Other(format!("invalid profiles response: {e}")))?;

    if json {
        crate::format::print_json(&resp)?;
    } else {
        print_profile_table(&resp);
    }

    Ok(())
}

/// Switch the active profile via the daemon.
pub async fn switch(client: &UdsClient, target: &str, json: bool) -> Result<(), CliError> {
    let mode: kikan_types::SetupMode = target.parse().map_err(|e: String| CliError::Other(e))?;

    let req = ProfileSwitchAdminRequest { profile: mode };
    let body = client.post("/profiles/switch", &req).await?;
    let resp: ProfileSwitchAdminResponse = serde_json::from_slice(&body)
        .map_err(|e| CliError::Other(format!("invalid switch response: {e}")))?;

    if json {
        crate::format::print_json(&resp)?;
    } else {
        println!(
            "Switched profile: {} \u{2192} {}",
            resp.previous, resp.current
        );
    }

    Ok(())
}

fn print_profile_table(resp: &ProfileListResponse) {
    println!(
        "{:<14} {:<8} {:<10} {:<12}",
        "Profile", "Active", "Schema", "Size"
    );
    println!("{}", crate::format::separator(46));
    for p in &resp.profiles {
        let active = if p.active { "*" } else { "" };
        let size = match p.file_size_bytes {
            Some(bytes) => format_bytes(bytes),
            None => "n/a".to_string(),
        };
        println!(
            "{:<14} {:<8} v{:<9} {:<12}",
            p.name, active, p.schema_version, size,
        );
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
