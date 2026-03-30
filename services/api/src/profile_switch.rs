use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum_login::AuthSession;
use mokumo_core::setup::SetupMode;
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::error::ErrorCode;
use mokumo_types::setup::{ProfileSwitchRequest, ProfileSwitchResponse};

use crate::SharedState;
use crate::auth::backend::Backend;
use crate::auth::user::AuthenticatedUser;
use crate::error::AppError;

/// POST /api/profile/switch — switch the active profile between demo and production.
///
/// Guards (N20–N26):
/// 1. Require auth — enforced by `require_auth_with_demo_auto_login` route layer.
/// 2. Rate limit: 3 switches per 15 minutes per user.
/// 3. Origin validation: Origin header must match the server's bound port or be a Tauri origin.
/// 4. Logout the current session.
/// 5. Look up the user in the target DB (demo → admin@demo.local; production → current email).
/// 6. Login the new user.
/// 7. Persist active_profile to disk.
/// 8. Update AppState.active_profile in memory.
/// 9. Return 200 ProfileSwitchResponse.
pub async fn profile_switch(
    State(state): State<SharedState>,
    mut auth_session: AuthSession<Backend>,
    headers: HeaderMap,
    Json(req): Json<ProfileSwitchRequest>,
) -> Result<Json<ProfileSwitchResponse>, AppError> {
    // Step 1: Auth enforced by layer; extract current user for rate-limit key and email lookup.
    let current_user = auth_session
        .user
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into()))?
        .clone();

    // Step 2: Rate limit — 3 switches per 15 minutes per user.
    if !state
        .switch_limiter
        .check_and_record(&current_user.user.id.to_string())
    {
        return Err(AppError::TooManyRequests(
            "Too many profile switch attempts. Try again later.".into(),
        ));
    }

    // Step 3: Origin validation — CSRF guard.
    let port = state.mdns_status.read().unwrap().port;
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_valid_origin(origin, port) {
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or missing Origin header".into(),
        ));
    }

    let target = req.profile;

    // Step 4: Logout current session.
    if let Err(e) = auth_session.logout().await {
        tracing::error!(user_id = %current_user.user.id, "Profile switch: logout failed: {e}");
        return Err(AppError::InternalError(
            "Failed to invalidate current session".into(),
        ));
    }

    // Step 5: Look up target user.
    let email = match target {
        SetupMode::Demo => "admin@demo.local".to_string(),
        SetupMode::Production => current_user.user.email.clone(),
    };
    let repo = SeaOrmUserRepo::new(state.db_for(target).clone());
    let (new_user_domain, hash) = repo.find_by_email_with_hash(&email).await?.ok_or_else(|| {
        tracing::error!(user_id = %current_user.user.id, target = ?target, %email, "Profile switch: target user not found in target DB");
        AppError::InternalError("Target profile is unavailable".into())
    })?;

    let new_user = AuthenticatedUser::new(new_user_domain, hash, target);

    // Step 6: Login with new profile session.
    if let Err(e) = auth_session.login(&new_user).await {
        tracing::error!("Profile switch: login failed: {e}");
        return Err(AppError::InternalError(
            "Failed to create new session".into(),
        ));
    }

    // Step 7: Persist active_profile to disk. This must succeed before we update in-memory state:
    // if the write fails and we proceed, a restart would load a stale active_profile file and
    // the server would start in the wrong profile.
    let profile_path = state.data_dir.join("active_profile");
    tokio::fs::write(&profile_path, target.as_str()).await.map_err(|e| {
        tracing::error!(user_id = %current_user.user.id, target = ?target, "Profile switch: failed to write active_profile file: {e}");
        AppError::InternalError("Failed to persist profile selection".into())
    })?;

    // Step 8: Update in-memory active_profile.
    *state.active_profile.write().unwrap() = target;

    // Mark first-launch as done on the first successful switch.
    let _ =
        state
            .is_first_launch
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed);

    // Step 9: Respond.
    Ok(Json(ProfileSwitchResponse { profile: target }))
}

/// Accept an Origin if it is a known Tauri desktop origin or a local/LAN origin on the correct
/// server port.
///
/// Empty/missing origins are rejected. Browser origins are parsed and checked on two dimensions:
/// 1. Port must exactly match the server's bound port.
/// 2. Host must be localhost, an mDNS `.local` hostname, or a private IP address.
///
/// The host check prevents DNS-rebinding attacks: without it, a foreign host on the correct port
/// (e.g. `http://evil.example.com:3000`) would pass a port-only check.
fn is_valid_origin(origin: &str, port: u16) -> bool {
    if origin.is_empty() {
        return false;
    }
    // Tauri v2 desktop origins — no port component.
    if origin == "tauri://localhost" || origin == "https://tauri.localhost" {
        return true;
    }
    // Browser/web origins: parse, then validate port + host.
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let (Some(host), Some(p)) = (url.host_str(), url.port()) else {
        return false;
    };
    p == port && is_local_host(host)
}

/// Return true for hosts that are definitively local: localhost, mDNS `.local` names, and
/// RFC-1918 / loopback IPv4 ranges.
fn is_local_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".local") {
        return true;
    }
    // Parse as IPv4 and check private / loopback ranges.
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        let octets = ip.octets();
        return ip.is_loopback()                                      // 127.x.x.x
            || octets[0] == 10                                        // 10.0.0.0/8
            || (octets[0] == 172 && (16..=31).contains(&octets[1])) // 172.16.0.0/12
            || (octets[0] == 192 && octets[1] == 168); // 192.168.0.0/16
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_origin() {
        assert!(!is_valid_origin("", 3000));
    }

    #[test]
    fn accepts_tauri_origins() {
        assert!(is_valid_origin("tauri://localhost", 3000));
        assert!(is_valid_origin("https://tauri.localhost", 3000));
    }

    #[test]
    fn accepts_matching_port() {
        assert!(is_valid_origin("http://localhost:3000", 3000));
        assert!(is_valid_origin("http://192.168.1.5:43210", 43210));
        assert!(is_valid_origin("http://shop.local:8080", 8080));
    }

    #[test]
    fn rejects_wrong_port() {
        assert!(!is_valid_origin("http://localhost:3001", 3000));
        assert!(!is_valid_origin("http://evil.example.com:80", 3000));
    }

    #[test]
    fn rejects_spoofed_origin_matching_port() {
        // A foreign host on the correct port must not be accepted.
        assert!(!is_valid_origin("http://evil.example.com:3000", 3000));
        assert!(!is_valid_origin("http://attacker.net:43210", 43210));
    }

    #[test]
    fn rejects_missing_port_non_tauri() {
        assert!(!is_valid_origin("http://localhost", 3000));
        assert!(!is_valid_origin("http://evil.example.com", 3000));
    }
}
