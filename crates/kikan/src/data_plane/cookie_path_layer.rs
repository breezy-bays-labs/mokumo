//! Defense-in-depth assertion that session cookies always carry `Path=/`.
//!
//! Per `adr-tauri-http-not-ipc.md` Commitment 7 (added 2026-04-24, M00
//! kikan admin UI shape pipeline), session cookies issued by the data
//! plane MUST have `Path = /`. Without this, the SPA at `/` silently
//! loses the session after a reload across the
//! `/admin/* ↔ /admin/{extensions,integrations}/{id}/*` boundary —
//! breaking the entire composed-origin shape that `adr-kikan-admin-ui.md`
//! §ADR-2 depends on.
//!
//! `tower-sessions` sets `Path=/` by default, but "by default" is fragile
//! as an invariant. A hook on every outbound `Set-Cookie` header whose
//! name matches the session cookie fires a `debug_assert!` if the `Path`
//! attribute is missing or not `/`. Debug builds and tests fail loudly;
//! release builds observe a `tracing::warn!` and continue (failing the
//! outbound cookie set would lock users out of a live install on a
//! library regression — noisier-but-degrading is the safer mode).
//!
//! The layer is pure-observation middleware — it does not mutate the
//! response. Contract enforcement lives upstream in the session layer's
//! cookie-builder configuration.

use axum::extract::Request;
use axum::http::HeaderName;
use axum::http::header::SET_COOKIE;
use axum::middleware::Next;
use axum::response::Response;

/// Name of the session cookie to monitor. `tower-sessions` defaults to
/// `"id"`; the data plane currently does not override it. If the default
/// ever changes the const tracks the new name.
const SESSION_COOKIE_NAME: &str = "id";

/// Middleware that asserts every outbound `Set-Cookie` for the session
/// cookie carries `Path=/`.
///
/// Compose as:
///
/// ```ignore
/// use axum::middleware::from_fn;
/// use kikan::data_plane::cookie_path_layer::assert_session_cookie_path_root;
///
/// let app = Router::new()
///     .route("/login", post(login_handler))
///     .layer(from_fn(assert_session_cookie_path_root));
/// ```
///
/// Applies to responses that carry the session `Set-Cookie` header;
/// responses without it pass through unchanged.
pub async fn assert_session_cookie_path_root(req: Request, next: Next) -> Response {
    let response = next.run(req).await;
    for set_cookie in response.headers().get_all(SET_COOKIE) {
        let Ok(value) = set_cookie.to_str() else {
            continue;
        };
        if !cookie_name_matches(value, SESSION_COOKIE_NAME) {
            continue;
        }
        if !has_path_root(value) {
            if cfg!(debug_assertions) {
                panic!(
                    "session cookie must carry Path=/ per adr-tauri-http-not-ipc \
                     Commitment 7; got Set-Cookie: {value}"
                );
            } else {
                tracing::warn!(
                    set_cookie = value,
                    "session cookie missing Path=/; SPA composed-origin navigation may break"
                );
            }
        }
    }
    response
}

/// Return `true` when the `Set-Cookie` header value names the given cookie.
///
/// Cookie spec: the leading token (before the first `=`) is the cookie
/// name. Whitespace trimming happens around the name.
fn cookie_name_matches(set_cookie: &str, expected_name: &str) -> bool {
    match set_cookie.split('=').next() {
        Some(name) => name.trim() == expected_name,
        None => false,
    }
}

/// Return `true` when the `Set-Cookie` header value contains `Path=/`
/// exactly (not `Path=/admin`, not `Path=/foo/`).
///
/// Attributes are separated by `;`. The `Path` attribute's value must be
/// the literal `/`.
fn has_path_root(set_cookie: &str) -> bool {
    set_cookie
        .split(';')
        .map(str::trim)
        .filter_map(|attr| {
            let (name, value) = attr.split_once('=')?;
            Some((name.trim(), value.trim()))
        })
        .any(|(name, value)| name.eq_ignore_ascii_case("Path") && value == "/")
}

/// The `HeaderName` the middleware monitors. Exposed so test harnesses and
/// diagnostic probes can reference it without re-constructing the constant.
pub const MONITORED_HEADER: HeaderName = SET_COOKIE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_name_matches_accepts_exact_leading_name() {
        assert!(cookie_name_matches("id=abc; Path=/; HttpOnly", "id"));
        assert!(cookie_name_matches(" id =abc; Path=/", "id"));
    }

    #[test]
    fn cookie_name_matches_rejects_other_cookies() {
        assert!(!cookie_name_matches("csrf=token; Path=/", "id"));
        assert!(!cookie_name_matches("identify=value; Path=/", "id"));
    }

    #[test]
    fn has_path_root_accepts_exact_root_path() {
        assert!(has_path_root("id=abc; Path=/; HttpOnly"));
        assert!(has_path_root("id=abc; HttpOnly; Path=/; SameSite=Lax"));
        assert!(has_path_root("id=abc; path=/"));
    }

    #[test]
    fn has_path_root_rejects_scoped_paths() {
        assert!(!has_path_root("id=abc; Path=/admin; HttpOnly"));
        assert!(!has_path_root("id=abc; Path=/foo/; HttpOnly"));
        assert!(!has_path_root("id=abc; Path=/admin/; HttpOnly"));
    }

    #[test]
    fn has_path_root_rejects_missing_path_attribute() {
        assert!(!has_path_root("id=abc; HttpOnly"));
        assert!(!has_path_root("id=abc"));
    }
}
