use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum_login::AuthSession;
use kikan::SetupMode;
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::error::ErrorCode;
use mokumo_types::setup::{ProfileSwitchRequest, ProfileSwitchResponse};

use crate::SharedState;
use crate::auth::backend::Backend;
use crate::auth::user::AuthenticatedUser;
use crate::error::AppError;

/// Session key used to carry the production user's email into the demo session so that a
/// subsequent demo→production switch can look up the correct account.
const SESSION_KEY_PRODUCTION_EMAIL: &str = "profile_switch.production_email";

/// POST /api/profile/switch — switch the active profile between demo and production.
///
/// Guards (N20–N26):
/// 1. Require auth — enforced by `require_auth_with_demo_auto_login` route layer.
/// 2. Rate limit: 3 switches per 15 minutes per user.
/// 3. Origin validation: Origin header must match the server's bound port and be a local or
///    Tauri origin.
/// 4. Look up the user in the target DB — before touching the session.
/// 5. Persist active_profile to disk — before touching the session.
/// 6. Update AppState.active_profile in memory.
/// 7. Logout the current session.
/// 8. Login the new user.
/// 9. Return 200 ProfileSwitchResponse.
///
/// Steps 4–6 occur before steps 7–8 deliberately: if target user lookup or disk write fail,
/// the current session is left intact and the caller gets a clean error.
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

    // Step 4: Resolve the target email.
    //
    // Production email across the demo session: when the user switches production→demo, we save
    // their production email in the session. On the next demo→production switch we read it back,
    // because at that point `current_user.user.email` is "admin@demo.local" — not the real
    // production account.
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

    // When switching to demo, capture the production email to re-insert after login so the
    // return trip (demo→production) can find the right account.
    let production_email_to_carry: Option<String> = match target {
        SetupMode::Demo => Some(
            // Prefer any previously saved email over the current user's email, in case the
            // current user is already the demo admin (demo→demo edge case).
            saved_production_email.unwrap_or_else(|| current_user.user.email.clone()),
        ),
        SetupMode::Production => None,
    };

    // Step 5: Look up the target user BEFORE any session changes. If the target account does not
    // exist (e.g. production not yet set up), we return early without logging the user out.
    let repo = SeaOrmUserRepo::new(state.db_for(target).clone());
    let (new_user_domain, hash) = repo.find_by_email_with_hash(&email).await?.ok_or_else(|| {
        tracing::error!(
            user_id = %current_user.user.id,
            target = ?target,
            %email,
            "Profile switch: target user not found in target DB"
        );
        AppError::ServiceUnavailable("Target profile is not available".into())
    })?;
    let new_user = AuthenticatedUser::new(new_user_domain, hash, target);

    // Step 6: Persist active_profile to disk BEFORE touching the session. Write to a temp file
    // in the same directory (same filesystem on POSIX = atomic rename), then rename over the
    // destination so a crash mid-write never leaves a partially-written file. If this fails we
    // return early — the current session and in-memory state are unchanged.
    let profile_path = state.data_dir.join("active_profile");
    let profile_tmp = state.data_dir.join("active_profile.tmp");
    tokio::fs::write(&profile_tmp, target.as_str())
        .await
        .map_err(|e| {
            tracing::error!(
                user_id = %current_user.user.id,
                target = ?target,
                "Profile switch: failed to write active_profile.tmp: {e}"
            );
            AppError::InternalError("Failed to persist profile selection".into())
        })?;
    tokio::fs::rename(&profile_tmp, &profile_path)
        .await
        .map_err(|e| {
            tracing::error!(
                user_id = %current_user.user.id,
                target = ?target,
                "Profile switch: failed to rename active_profile.tmp → active_profile: {e}"
            );
            AppError::InternalError("Failed to persist profile selection".into())
        })?;

    // Step 7: Update in-memory active_profile. Capture previous value for rollback if the session
    // operations below fail.
    let previous_profile = {
        let mut guard = state.active_profile.write();
        let prev = *guard;
        *guard = target;
        prev
    };

    // Step 8: Logout and login.
    //
    // On failure, roll back the in-memory active_profile and make a best-effort attempt to
    // restore the disk file. Rollback errors are logged but not propagated — the original
    // failure is what the caller needs to see.
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
    if let Some(ref prod_email) = production_email_to_carry {
        let insert_result = auth_session
            .session
            .insert(SESSION_KEY_PRODUCTION_EMAIL, prod_email.clone())
            .await;
        if let Err(e) = insert_result {
            tracing::warn!("Profile switch: failed to persist production_email in session: {e}");
        }
    }

    // When switching to demo, re-validate the demo install so that
    // /api/health and the 423 guard reflect the current demo DB state.
    // (Production never needs validation — an empty production DB is valid.)
    if target == SetupMode::Demo {
        let ok = mokumo_db::validate_installation(&state.demo_db).await;
        state.demo_install_ok.store(ok, Ordering::Release);
    }

    // Mark first-launch as done on the first successful switch.
    // Idempotent: if already false, the CAS is a harmless no-op.
    // Relaxed failure ordering because the result is discarded.
    let _ =
        state
            .is_first_launch
            .compare_exchange(true, false, Ordering::Release, Ordering::Relaxed);

    Ok(Json(ProfileSwitchResponse { profile: target }))
}

/// Accept an Origin if it is a known Tauri desktop origin or a local/LAN origin on the correct
/// server port.
///
/// The check has two layers:
///
/// 1. **Port** — `url::Url::port_or_known_default()` is used so that implicit ports (80 for
///    `http://`, 443 for `https://`) are treated the same as explicit ones.
///
/// 2. **Host** — compared against the request's `Host` header (exact authority match) when
///    present; falls back to `is_local_host` when the `Host` header is absent. The Host header
///    comparison prevents DNS-rebinding: a foreign host on the correct port (e.g.
///    `http://evil.example.com:3000`) cannot forge a matching authority.
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
    // url::Url::host_str() wraps IPv6 addresses in brackets (e.g. "[::1]"). Strip them so that
    // downstream comparisons and the contains(':') check for re-adding brackets work correctly.
    let host = raw_host.trim_start_matches('[').trim_end_matches(']');
    // port_or_known_default() treats http://localhost as http://localhost:80, so a server bound
    // to port 80 correctly accepts origins without an explicit port component.
    let Some(p) = url.port_or_known_default() else {
        return false;
    };
    if p != port {
        return false;
    }

    if !request_host.is_empty() {
        // Compare the origin authority against the request's Host header. The Host header uses
        // bracket notation for IPv6 (e.g. [::1]:3000); url::Url::host_str() strips the brackets,
        // so we re-add them when formatting the origin authority.
        let origin_authority = if host.contains(':') {
            // IPv6 address — wrap in brackets to match the Host header format.
            format!("[{host}]:{p}")
        } else {
            format!("{host}:{p}")
        };
        // Also accept when the Host header omits the port (scheme-default ports).
        return origin_authority == request_host || host == request_host;
    }

    is_local_host(host)
}

/// Return true for hosts that are definitively local: localhost, IPv6 loopback (`::1`), mDNS
/// `.local` names, RFC-1918 / loopback IPv4 ranges, and IPv6 loopback.
///
/// Used as a fallback when the `Host` header is absent.
fn is_local_host(host: &str) -> bool {
    if host == "localhost" || host == "::1" || host.ends_with(".local") {
        return true;
    }
    // Parse as IPv4 and check private / loopback ranges.
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        let o = ip.octets();
        return ip.is_loopback() // 127.x.x.x
            || o[0] == 10 // 10.0.0.0/8
            || (o[0] == 172 && (16..=31).contains(&o[1])) // 172.16.0.0/12
            || (o[0] == 192 && o[1] == 168); // 192.168.0.0/16
    }
    // IPv6 loopback (::1 is already handled above; this catches any future canonical form).
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
        // port_or_known_default() maps http://host → port 80; server on port 80 should accept it.
        assert!(is_valid_origin("http://localhost", 80, ""));
    }

    #[test]
    fn rejects_wrong_port() {
        assert!(!is_valid_origin("http://localhost:3001", 3000, ""));
        assert!(!is_valid_origin("http://evil.example.com:80", 3000, ""));
    }

    #[test]
    fn rejects_missing_port_non_tauri() {
        // http://localhost has implicit port 80; does not match a server on port 3000.
        assert!(!is_valid_origin("http://localhost", 3000, ""));
        assert!(!is_valid_origin("http://evil.example.com", 3000, ""));
    }

    #[test]
    fn rejects_spoofed_origin_matching_port() {
        // A foreign host on the correct port must not be accepted via is_local_host fallback.
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
        // evil.example.com matches the port but not the Host authority.
        assert!(!is_valid_origin(
            "http://evil.example.com:3000",
            3000,
            "localhost:3000"
        ));
    }

    #[test]
    fn accepts_ipv6_origin_matching_host_header() {
        // Host header uses bracket notation; url::Url::host_str() strips them.
        assert!(is_valid_origin("http://[::1]:3000", 3000, "[::1]:3000"));
    }
}
