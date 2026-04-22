//! Mode-aware session cookie configuration.

use time::Duration;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite};
use tower_sessions_sqlx_store::SqliteStore;

use super::DeploymentMode;
use crate::engine::Sessions;

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
        .with_secure(secure)
        .with_http_only(true)
        .with_same_site(same_site)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // SessionManagerLayer doesn't expose its cookie config for inspection, so
    // these tests lock the per-mode branch at the function boundary only. The
    // composition test in `tests/data_plane_modes.rs` covers the observable
    // Set-Cookie output.

    #[test]
    fn all_three_modes_build_without_panic() {
        // Build a throwaway store so we can exercise the builder.
        use tokio::runtime::Runtime;
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
            let store = SqliteStore::new(pool);
            let sessions = Sessions::new(store);
            for mode in [
                DeploymentMode::Lan,
                DeploymentMode::Internet,
                DeploymentMode::ReverseProxy,
            ] {
                let _ = session_layer_for_mode(&sessions, mode);
            }
        });
    }
}
