//! Per-request database handle, selected by session profile.
//!
//! `ProfileDb` is inserted into request extensions by
//! [`profile_db_middleware`], which runs immediately after
//! `AuthManagerLayer` and reads the compound session id
//! `ProfileUserId(mode, _)` to pick the correct database. Handlers in
//! protected routes extract the handle via `ProfileDb(db): ProfileDb`,
//! ensuring every request sees the database chosen by its own session —
//! not a snapshot captured at router-build time. This is what preserves
//! seamless profile switching: no restart, no cross-profile bleed, and
//! no handler code paths that can silently bind to the wrong database.
//!
//! Both the type and the middleware live in kikan: the middleware only
//! touches kikan surfaces (`PlatformState`, `Backend`, `ProfileUserId`),
//! so no adapter shell needs to own it. The vertical wires the middleware
//! at its mount site with `from_fn_with_state(state.platform_state(), …)`.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{StatusCode, request::Parts};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_login::{AuthSession, AuthUser};

use crate::auth::{Backend, ProfileUserId};
use crate::db::DatabaseConnection;
use crate::platform_state::PlatformState;
use crate::tenancy::SetupMode;

/// Per-request database handle injected by the vertical's profile-routing
/// middleware.
///
/// Wraps `DatabaseConnection` directly — `sea_orm::DatabaseConnection` is
/// already Arc-backed internally, so no additional `Arc` wrapper is needed.
///
/// Handlers on protected routes extract this instead of going through a
/// router-level `State<_>` handle, ensuring each request always uses the
/// correct profile database regardless of the current global "active
/// profile" setting.
#[derive(Clone, Debug)]
pub struct ProfileDb(pub DatabaseConnection);

impl ProfileDb {
    /// Borrow the inner database connection.
    pub fn inner(&self) -> &DatabaseConnection {
        &self.0
    }
}

impl<S> FromRequestParts<S> for ProfileDb
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ProfileDb>()
            .cloned()
            .ok_or_else(missing_extension_response)
    }
}

/// Per-request view of the request's effective profile (Demo / Production).
///
/// Inserted into request extensions by the same middleware that provides
/// `ProfileDb`. Handlers with profile-gated policy (e.g. logo management
/// requires Production) extract this instead of reaching into a shared
/// `AppState`. Keeping the extractor in kikan lets vertical crates
/// enforce profile policy without depending on the shell.
#[derive(Clone, Copy, Debug)]
pub struct ActiveProfile(pub SetupMode);

impl<S> FromRequestParts<S> for ActiveProfile
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ActiveProfile>()
            .copied()
            .ok_or_else(missing_extension_response)
    }
}

/// Profile-routing middleware: inject `ProfileDb` + `ActiveProfile` into
/// request extensions based on session profile.
///
/// Must be placed AFTER `AuthManagerLayer` in the layer stack (innermost)
/// so that the auth session is already populated when this runs.
///
/// - **Authenticated request**: reads `(mode, _)` from
///   `auth_session.user.id()` and inserts the corresponding database.
/// - **Unauthenticated request**: falls back to
///   `platform.active_profile` — the currently active profile snapshot.
///
/// Wired at the mount site with
/// `from_fn_with_state(state.platform_state(), profile_db_middleware)`.
pub async fn profile_db_middleware(
    State(platform): State<PlatformState>,
    auth_session: AuthSession<Backend>,
    mut request: Request,
    next: Next,
) -> Response {
    use std::str::FromStr;
    let (mode, db) = if let Some(user) = &auth_session.user {
        let ProfileUserId(m, _) = user.id();
        let pool = platform
            .db_for(m.as_dir_name())
            .cloned()
            .expect("authenticated session references a profile without a pool");
        (m, pool)
    } else {
        let active = platform.active_profile.read().clone();
        let m = SetupMode::from_str(active.as_str()).unwrap_or(SetupMode::Demo);
        let pool = platform
            .db_for(active.as_str())
            .cloned()
            .expect("active profile references a pool entry");
        (m, pool)
    };

    request.extensions_mut().insert(ProfileDb(db));
    request.extensions_mut().insert(ActiveProfile(mode));
    next.run(request).await
}

/// Build a 500 response whose body matches the platform-wide `ErrorBody`
/// wire shape (`{"code":"internal_error","message":...,"details":null}`)
/// without pulling `kikan-types` into `kikan` (kikan-types already depends
/// on kikan for `SetupMode`; the dependency cannot reverse).
///
/// The message is intentionally generic — reaching this arm means the
/// vertical forgot to install its profile-routing middleware, which is a
/// programmer error, not a user-facing condition. The status code + code
/// string are the load-bearing parts.
fn missing_extension_response() -> Response {
    let body = serde_json::json!({
        "code": "internal_error",
        "message": "An internal error occurred",
        "details": null,
    });
    let mut response = (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(body)).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "no-store".parse().expect("static header value parses"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn from_request_parts_returns_err_when_extension_absent() {
        use axum::http::Request;

        let req = Request::builder().body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = ProfileDb::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn rejection_body_matches_platform_error_wire_shape() {
        use axum::http::Request;

        let req = Request::builder().body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let response = ProfileDb::from_request_parts(&mut parts, &())
            .await
            .expect_err("missing extension must reject");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "internal_error");
        assert_eq!(json["message"], "An internal error occurred");
        assert!(json["details"].is_null());
    }

    #[tokio::test]
    async fn active_profile_extractor_rejects_when_missing() {
        use axum::http::Request;

        let req = Request::builder().body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = ActiveProfile::from_request_parts(&mut parts, &()).await;
        assert_eq!(
            result.unwrap_err().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn active_profile_extractor_returns_inserted_value() {
        use axum::http::Request;

        let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
        req.extensions_mut().insert(ActiveProfile(SetupMode::Demo));
        let (mut parts, _) = req.into_parts();

        let ActiveProfile(mode) = ActiveProfile::from_request_parts(&mut parts, &())
            .await
            .expect("extractor should succeed when extension present");
        assert_eq!(mode, SetupMode::Demo);

        let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ActiveProfile(SetupMode::Production));
        let (mut parts, _) = req.into_parts();

        let ActiveProfile(mode) = ActiveProfile::from_request_parts(&mut parts, &())
            .await
            .expect("extractor should succeed when extension present");
        assert_eq!(mode, SetupMode::Production);
    }

    /// Verify that from_request_parts returns the exact ProfileDb that was
    /// inserted, and that two distinct databases inserted for demo vs
    /// production sessions are correctly routed — the extracted handle
    /// queries the intended database.
    #[tokio::test]
    async fn routing_returns_correct_db_per_profile() {
        use crate::db::{DatabaseConnection, initialize_database};
        use axum::http::Request;

        async fn user_version(db: &DatabaseConnection) -> i64 {
            let pool = db.get_sqlite_connection_pool();
            sqlx::query_scalar::<_, i64>("PRAGMA user_version")
                .fetch_one(pool)
                .await
                .expect("user_version query failed")
        }

        async fn set_user_version(db: &DatabaseConnection, v: i64) {
            let pool = db.get_sqlite_connection_pool();
            sqlx::query(&format!("PRAGMA user_version = {v}"))
                .execute(pool)
                .await
                .expect("set user_version failed");
        }

        let demo_db = initialize_database("sqlite::memory:?mode=rwc")
            .await
            .unwrap();
        let prod_db = initialize_database("sqlite::memory:?mode=rwc")
            .await
            .unwrap();

        set_user_version(&demo_db, 1).await;
        set_user_version(&prod_db, 2).await;

        // Demo session
        let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
        req.extensions_mut().insert(ProfileDb(demo_db));
        let (mut parts, _) = req.into_parts();
        let ProfileDb(extracted) = ProfileDb::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(
            user_version(&extracted).await,
            1,
            "demo session should use the demo DB"
        );

        // Production session
        let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
        req.extensions_mut().insert(ProfileDb(prod_db));
        let (mut parts, _) = req.into_parts();
        let ProfileDb(extracted) = ProfileDb::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(
            user_version(&extracted).await,
            2,
            "production session should use the production DB"
        );
    }
}
