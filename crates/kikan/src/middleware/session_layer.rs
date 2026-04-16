use time::Duration;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite};
use tower_sessions_sqlx_store::SqliteStore;

use crate::engine::Sessions;

/// Construct the platform session layer, matching the composition used by
/// `services/api::build_app_inner`:
/// - `secure = false` — M0 runs LAN HTTP, not HTTPS.
/// - `http_only = true` — JS cannot read the session cookie.
/// - `SameSite=Lax` — bookmarks and mDNS links preserve the session.
/// - `Expiry::OnInactivity(24h)`.
pub fn session_layer(sessions: &Sessions) -> SessionManagerLayer<SqliteStore> {
    SessionManagerLayer::new(sessions.store())
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)))
}
