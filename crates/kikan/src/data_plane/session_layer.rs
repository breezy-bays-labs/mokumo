//! Mode-aware session cookie configuration.

use time::Duration;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite};
use tower_sessions_sqlx_store::SqliteStore;

use super::DeploymentMode;
use crate::engine::Sessions;

/// Name of the session cookie. Single source of truth shared with
/// [`crate::data_plane::cookie_path_layer`] so the defense-in-depth `Path=/`
/// assertion can never silently no-op due to a name drift.
///
/// Matches the `tower-sessions` default; pinned here explicitly via
/// [`SessionManagerLayer::with_name`] in [`session_layer_for_mode`] so the
/// invariant survives an upstream default change.
pub const SESSION_COOKIE_NAME: &str = "id";

/// Session layer with cookie flags selected from [`DeploymentMode`].
///
/// - **Lan**: `Secure=false`, `SameSite=Lax`. LAN runs HTTP, and `Lax` keeps
///   bookmarked / mDNS-shared links working.
/// - **Internet** / **ReverseProxy**: `Secure=true`, `SameSite=Strict`. The
///   browser refuses to send the cookie over plain HTTP and blocks cross-site
///   navigations from attaching it.
///
/// `http_only` is always `true` (JS cannot read the cookie). Expiry is 24h of
/// inactivity in every mode.
pub fn session_layer_for_mode(
    sessions: &Sessions,
    mode: DeploymentMode,
) -> SessionManagerLayer<SqliteStore> {
    let (secure, same_site) = match mode {
        DeploymentMode::Lan => (false, SameSite::Lax),
        DeploymentMode::Internet | DeploymentMode::ReverseProxy => (true, SameSite::Strict),
    };

    SessionManagerLayer::new(sessions.store())
        .with_name(SESSION_COOKIE_NAME)
        .with_secure(secure)
        .with_http_only(true)
        .with_same_site(same_site)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::header::SET_COOKIE;
    use http::{Request, Response, StatusCode};
    use std::convert::Infallible;
    use tower::{ServiceBuilder, ServiceExt};
    use tower_sessions::Session;

    // `SessionManagerLayer` doesn't expose its cookie config for inspection,
    // so each test exercises the layer end-to-end: touch the session in a
    // downstream handler so the store flushes, then inspect the `Set-Cookie`
    // header on the outgoing response. The invariants pinned here are:
    //
    // - `HttpOnly` is always set (JS must never read the session cookie).
    // - `Secure` and `SameSite` vary by mode per the doc comment above.
    //
    // `store.migrate()` creates the `tower_sessions` table the session flush
    // writes into. `max_connections(1)` pins the pool to a single connection
    // so migrate + flush share the same `sqlite::memory:` database — without
    // it, SQLite hands each connection its own private in-memory DB and the
    // flush hits a "no such table" error on a different connection than
    // migrate ran on.
    async fn sessions_for_test() -> Sessions {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let store = SqliteStore::new(pool);
        store.migrate().await.unwrap();
        Sessions::new(store)
    }

    /// Split a `Set-Cookie` header into its attribute tokens, skipping the
    /// opaque `name=value` pair. Whole-token comparison prevents a false pass
    /// from the session ID happening to contain `Secure` or `HttpOnly` as a
    /// substring. Attribute names are matched case-insensitively per RFC 6265.
    fn has_cookie_attr(set_cookie: &str, expected: &str) -> bool {
        set_cookie
            .split(';')
            .skip(1)
            .map(str::trim)
            .any(|attr| attr.eq_ignore_ascii_case(expected))
    }

    async fn touch_handler(req: Request<Body>) -> Result<Response<Body>, Infallible> {
        let session = req
            .extensions()
            .get::<Session>()
            .expect("session extension missing — layer stack is wrong");
        session
            .insert("touch", true)
            .await
            .expect("session insert into in-memory store must not fail");
        Ok(Response::new(Body::empty()))
    }

    async fn set_cookie_for(mode: DeploymentMode) -> String {
        let sessions = sessions_for_test().await;
        let layer = session_layer_for_mode(&sessions, mode);
        let svc = ServiceBuilder::new().layer(layer).service_fn(touch_handler);
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        resp.headers()
            .get(SET_COOKIE)
            .expect("touch handler modified session — Set-Cookie must be present")
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[tokio::test]
    async fn lan_mode_emits_insecure_lax_httponly_cookie() {
        let set_cookie = set_cookie_for(DeploymentMode::Lan).await;
        assert!(
            has_cookie_attr(&set_cookie, "HttpOnly"),
            "HttpOnly invariant: cookie must be HttpOnly in every mode; got: {set_cookie}"
        );
        assert!(
            !has_cookie_attr(&set_cookie, "Secure"),
            "Lan runs HTTP; Secure would break the cookie on a plain-HTTP LAN; got: {set_cookie}"
        );
        assert!(
            has_cookie_attr(&set_cookie, "SameSite=Lax"),
            "Lan uses SameSite=Lax so mDNS-shared / bookmarked links keep working; got: {set_cookie}"
        );
    }

    #[tokio::test]
    async fn internet_mode_emits_secure_strict_httponly_cookie() {
        let set_cookie = set_cookie_for(DeploymentMode::Internet).await;
        assert!(
            has_cookie_attr(&set_cookie, "HttpOnly"),
            "HttpOnly invariant: cookie must be HttpOnly in every mode; got: {set_cookie}"
        );
        assert!(
            has_cookie_attr(&set_cookie, "Secure"),
            "Internet mode assumes HTTPS at the socket; Secure must be set; got: {set_cookie}"
        );
        assert!(
            has_cookie_attr(&set_cookie, "SameSite=Strict"),
            "Internet mode uses SameSite=Strict; got: {set_cookie}"
        );
    }

    #[tokio::test]
    async fn reverse_proxy_mode_emits_secure_strict_httponly_cookie() {
        let set_cookie = set_cookie_for(DeploymentMode::ReverseProxy).await;
        assert!(
            has_cookie_attr(&set_cookie, "HttpOnly"),
            "HttpOnly invariant: cookie must be HttpOnly in every mode; got: {set_cookie}"
        );
        assert!(
            has_cookie_attr(&set_cookie, "Secure"),
            "ReverseProxy assumes HTTPS at the proxy; Secure must be set; got: {set_cookie}"
        );
        assert!(
            has_cookie_attr(&set_cookie, "SameSite=Strict"),
            "ReverseProxy uses SameSite=Strict; got: {set_cookie}"
        );
    }

    #[tokio::test]
    async fn session_cookie_name_matches_shared_constant() {
        // Pins the contract that `cookie_path_layer` assumes: the outbound
        // session cookie's leading `name=value` token uses
        // `SESSION_COOKIE_NAME`. If `with_name(...)` is removed or the const
        // drifts, the defense-in-depth Path=/ middleware silently no-ops.
        let set_cookie = set_cookie_for(DeploymentMode::Lan).await;
        let name = set_cookie
            .split('=')
            .next()
            .expect("Set-Cookie must have a name=value pair")
            .trim();
        assert_eq!(
            name, SESSION_COOKIE_NAME,
            "session cookie name must match the shared SESSION_COOKIE_NAME const; got: {set_cookie}"
        );
    }

    #[test]
    fn has_cookie_attr_matches_whole_token_only() {
        // The opaque `id=<value>` pair is skipped so a value like
        // `id=Securely-random` cannot satisfy a `Secure` check.
        let h = "id=Securely-random; HttpOnly; SameSite=Lax";
        assert!(has_cookie_attr(h, "HttpOnly"));
        assert!(has_cookie_attr(h, "SameSite=Lax"));
        assert!(!has_cookie_attr(h, "Secure"));
        // Attribute names are case-insensitive per RFC 6265.
        assert!(has_cookie_attr(h, "httponly"));
        assert!(has_cookie_attr(h, "SAMESITE=Lax"));
    }
}
