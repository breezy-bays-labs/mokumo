//! POST /api/profile/switch — switch the active profile between demo and production.
//!
//! ## Split: adapter vs. pure control-plane fn
//!
//! This module is the **HTTP adapter** for profile switching. It owns the
//! transport-coupled concerns that cannot live inside a pure fn:
//!
//! - Rate limiting (3 switches / 15 min per user).
//! - CSRF/Origin validation.
//! - Email resolution from the active tower-session.
//! - Session logout (`auth_session.logout`) and login (`auth_session.login`).
//! - SESSION_KEY_PRODUCTION_EMAIL carry-over across the demo↔production boundary.
//! - demo_install_ok revalidation after a switch into the demo profile.
//! - is_first_launch CAS after the first successful switch.
//!
//! The three persistence operations that are transport-neutral (user lookup in
//! target DB, atomic disk persist of `active_profile`, in-memory flip of
//! `PlatformState::active_profile`) are delegated to
//! [`kikan::control_plane::profiles::switch_profile`]. That fn returns a
//! [`kikan::control_plane::profiles::SwitchOutcome`] carrying the
//! `AuthenticatedUser` we pass to `auth_session.login` and the
//! `previous_profile` needed for rollback if the session operations fail.
//!
//! ## LOC note
//!
//! This file is intentionally larger than the 150-LOC target from the Wave-E
//! spec ([CEng-10]). The `is_valid_origin` + `is_local_host` helpers and their
//! 30+ unit tests (~170 LOC) are kept here rather than extracted to a sibling
//! module because they serve no other call site today. The handler body itself
//! is ~70 LOC.

use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum_login::AuthSession;
use kikan::SetupMode;
use kikan_types::error::ErrorCode;
use kikan_types::setup::{ProfileSwitchRequest, ProfileSwitchResponse};

use crate::SharedState;
use crate::error::AppError;
use kikan::auth::Backend;

/// Session key used to carry the production user's email into the demo session
/// so that a subsequent demo→production switch can look up the correct account.
const SESSION_KEY_PRODUCTION_EMAIL: &str = "profile_switch.production_email";

/// POST /api/profile/switch
///
/// Guards (N20–N26):
/// - Require auth — enforced by `require_auth_with_demo_auto_login` route layer.
/// - Rate limit: 3 switches per 15 minutes per user.
/// - Origin validation: Origin header must match the server's bound port and be a local or Tauri origin.
/// - Pure-fn: look up target user + disk-persist + memory flip via `kikan::control_plane::profiles::switch_profile`.
/// - Logout the current session.
/// - Login the new user.
///
/// The pure-fn steps occur before session operations deliberately: if target user lookup
/// or disk write fail, the current session is left intact and the caller gets a clean error.
pub async fn profile_switch(
    State(state): State<SharedState>,
    mut auth_session: AuthSession<Backend>,
    headers: HeaderMap,
    Json(req): Json<ProfileSwitchRequest>,
) -> Result<Json<ProfileSwitchResponse>, AppError> {
    // Step 1: Auth enforced by layer; extract current user for rate-limit key
    // and email carry-over.
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
    let port = state.mdns_status.read().port;
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let request_host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_valid_origin(origin, port, request_host) {
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or missing Origin header".into(),
        ));
    }

    let target = req.profile;

    // Resolve the target email from the active session.
    //
    // Production email across the demo session: when the user switches
    // production→demo, we save their production email in the session. On the
    // next demo→production switch we read it back, because at that point
    // `current_user.user.email` is "admin@demo.local" — not the real account.
    let saved_production_email: Option<String> = auth_session
        .session
        .get::<String>(SESSION_KEY_PRODUCTION_EMAIL)
        .await
        .ok()
        .flatten();

    let email = match target {
        SetupMode::Demo => "admin@demo.local".to_string(),
        SetupMode::Production => saved_production_email
            .clone()
            .unwrap_or_else(|| current_user.user.email.clone()),
    };

    // When switching to demo, capture the production email for the return trip.
    let production_email_to_carry: Option<String> = match target {
        SetupMode::Demo => {
            Some(saved_production_email.unwrap_or_else(|| current_user.user.email.clone()))
        }
        SetupMode::Production => None,
    };

    // Steps 4–6: pure-fn — user lookup in target DB, disk persist, memory flip.
    //
    // `platform_state()` is O(1): all PlatformState fields are Arc-backed so
    // the projection is just reference-count increments.
    //
    // Maps NotFound → 503 to preserve the existing wire behaviour ("target
    // profile is not available"), rather than the default 404 mapping.
    let platform = state.platform_state();
    let kikan::control_plane::profiles::SwitchOutcome {
        new_user,
        previous_profile,
    } = kikan::control_plane::profiles::switch_profile(&platform, target, &email)
        .await
        .map_err(|e| match e {
            kikan::ControlPlaneError::NotFound => {
                tracing::error!(
                    user_id = %current_user.user.id,
                    target = ?target,
                    %email,
                    "Profile switch: target user not found in target DB"
                );
                AppError::ServiceUnavailable("Target profile is not available".into())
            }
            other => AppError::from(other),
        })?;

    // Step 7: Logout and login.
    //
    // On failure, roll back the in-memory active_profile and make a
    // best-effort attempt to restore the disk file. Rollback errors are
    // logged but not propagated — the original failure is what the caller
    // needs.
    let profile_path = state.data_dir.join("active_profile");
    let profile_tmp = state.data_dir.join("active_profile.tmp");

    if let Err(e) = auth_session.logout().await {
        tracing::error!(
            user_id = %current_user.user.id,
            target = ?target,
            "Profile switch: logout failed — rolling back active_profile: {e}"
        );
        *state.active_profile.write() = previous_profile;
        if let Err(re) = async {
            tokio::fs::write(&profile_tmp, previous_profile.as_str()).await?;
            tokio::fs::rename(&profile_tmp, &profile_path).await
        }
        .await
        {
            tracing::error!(
                path = %profile_path.display(),
                "Profile switch: rollback disk write failed — on-disk profile may be inconsistent: {re}"
            );
        }
        return Err(AppError::InternalError(
            "Failed to invalidate current session".into(),
        ));
    }
    if let Err(e) = auth_session.login(&new_user).await {
        tracing::error!(
            user_id = %current_user.user.id,
            target = ?target,
            "Profile switch: login failed — rolling back active_profile: {e}"
        );
        *state.active_profile.write() = previous_profile;
        if let Err(re) = async {
            tokio::fs::write(&profile_tmp, previous_profile.as_str()).await?;
            tokio::fs::rename(&profile_tmp, &profile_path).await
        }
        .await
        {
            tracing::error!(
                path = %profile_path.display(),
                "Profile switch: rollback disk write failed — on-disk profile may be inconsistent: {re}"
            );
        }
        return Err(AppError::InternalError(
            "Failed to create new session".into(),
        ));
    }

    // Persist the production email into the new session for the return trip.
    if let Some(ref prod_email) = production_email_to_carry
        && let Err(e) = auth_session
            .session
            .insert(SESSION_KEY_PRODUCTION_EMAIL, prod_email.clone())
            .await
    {
        tracing::warn!("Profile switch: failed to persist production_email in session: {e}");
    }

    // When switching to demo, re-validate the demo install so that /api/health
    // and the 423 guard reflect the current demo DB state.
    if target == SetupMode::Demo {
        let ok = kikan::db::validate_installation(&state.demo_db).await;
        state.demo_install_ok.store(ok, Ordering::Release);
    }

    // Mark first-launch as done on the first successful switch. Idempotent.
    let _ =
        state
            .is_first_launch
            .compare_exchange(true, false, Ordering::Release, Ordering::Relaxed);

    Ok(Json(ProfileSwitchResponse { profile: target }))
}

/// Accept an Origin if it is a known Tauri desktop origin or a local/LAN
/// origin on the correct server port.
///
/// The check has two layers:
///
/// 1. **Port** — `url::Url::port_or_known_default()` is used so that implicit
///    ports (80 for `http://`, 443 for `https://`) are treated the same as
///    explicit ones.
///
/// 2. **Host** — compared against the request's `Host` header (exact authority
///    match) when present; falls back to `is_local_host` when the `Host`
///    header is absent. The Host header comparison prevents DNS-rebinding: a
///    foreign host on the correct port cannot forge a matching authority.
fn is_valid_origin(origin: &str, port: u16, request_host: &str) -> bool {
    if origin.is_empty() {
        return false;
    }
    // Tauri v2 desktop origins — no port component.
    if origin == "tauri://localhost" || origin == "https://tauri.localhost" {
        return true;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let Some(raw_host) = url.host_str() else {
        return false;
    };
    // url::Url::host_str() wraps IPv6 addresses in brackets (e.g. "[::1]").
    // Strip them so that downstream comparisons work correctly.
    let host = raw_host.trim_start_matches('[').trim_end_matches(']');
    let Some(p) = url.port_or_known_default() else {
        return false;
    };
    if p != port {
        return false;
    }

    if !request_host.is_empty() {
        // Compare the origin authority against the request's Host header.
        let origin_authority = if host.contains(':') {
            format!("[{host}]:{p}")
        } else {
            format!("{host}:{p}")
        };
        return origin_authority == request_host || host == request_host;
    }

    is_local_host(host)
}

/// Return true for hosts that are definitively local: localhost, IPv6 loopback,
/// mDNS `.local` names, RFC-1918 / loopback IPv4 ranges, and IPv6 loopback.
///
/// Used as a fallback when the `Host` header is absent.
fn is_local_host(host: &str) -> bool {
    if host == "localhost" || host == "::1" || host.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        let o = ip.octets();
        return ip.is_loopback()
            || o[0] == 10
            || (o[0] == 172 && (16..=31).contains(&o[1]))
            || (o[0] == 192 && o[1] == 168);
    }
    if let Ok(ip) = host.parse::<std::net::Ipv6Addr>() {
        return ip.is_loopback();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Tauri origins ---

    #[test]
    fn accepts_tauri_origins() {
        assert!(is_valid_origin("tauri://localhost", 3000, ""));
        assert!(is_valid_origin("https://tauri.localhost", 3000, ""));
    }

    // --- Port matching (no Host header fallback path) ---

    #[test]
    fn rejects_empty_origin() {
        assert!(!is_valid_origin("", 3000, ""));
    }

    #[test]
    fn accepts_matching_port() {
        assert!(is_valid_origin("http://localhost:3000", 3000, ""));
        assert!(is_valid_origin("http://192.168.1.5:43210", 43210, ""));
        assert!(is_valid_origin("http://shop.local:8080", 8080, ""));
    }

    #[test]
    fn accepts_default_http_port() {
        assert!(is_valid_origin("http://localhost", 80, ""));
    }

    #[test]
    fn rejects_wrong_port() {
        assert!(!is_valid_origin("http://localhost:3001", 3000, ""));
        assert!(!is_valid_origin("http://evil.example.com:80", 3000, ""));
    }

    #[test]
    fn rejects_missing_port_non_tauri() {
        assert!(!is_valid_origin("http://localhost", 3000, ""));
        assert!(!is_valid_origin("http://evil.example.com", 3000, ""));
    }

    #[test]
    fn rejects_spoofed_origin_matching_port() {
        assert!(!is_valid_origin("http://evil.example.com:3000", 3000, ""));
        assert!(!is_valid_origin("http://attacker.net:43210", 43210, ""));
    }

    // --- IPv6 ---

    #[test]
    fn accepts_ipv6_loopback() {
        assert!(is_valid_origin("http://[::1]:3000", 3000, ""));
    }

    // --- Host header path ---

    #[test]
    fn accepts_origin_matching_host_header() {
        assert!(is_valid_origin(
            "http://localhost:3000",
            3000,
            "localhost:3000"
        ));
        assert!(is_valid_origin(
            "http://192.168.1.5:8080",
            8080,
            "192.168.1.5:8080"
        ));
    }

    #[test]
    fn rejects_spoofed_origin_when_host_header_present() {
        assert!(!is_valid_origin(
            "http://evil.example.com:3000",
            3000,
            "localhost:3000"
        ));
    }

    #[test]
    fn accepts_ipv6_origin_matching_host_header() {
        assert!(is_valid_origin("http://[::1]:3000", 3000, "[::1]:3000"));
    }
}
