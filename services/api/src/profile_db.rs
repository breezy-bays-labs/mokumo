//! Per-request database handle, selected by session profile.
//!
//! `ProfileDbMiddleware` runs immediately after `AuthManagerLayer`. For
//! authenticated requests it reads the profile discriminant from the compound
//! user ID `(SetupMode, i64)` and inserts `ProfileDb` into request extensions.
//! For unauthenticated requests it falls back to `AppState.active_profile`.
//!
//! Protected handlers extract the handle via `ProfileDb(db): ProfileDb`.

use axum::extract::FromRequestParts;
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_login::AuthSession;
use axum_login::AuthUser;
use mokumo_db::DatabaseConnection;

use crate::SharedState;
use crate::auth::backend::Backend;
use crate::auth::user::ProfileUserId;
use crate::error::AppError;

/// Per-request database handle injected by `ProfileDbMiddleware`.
///
/// Wraps `DatabaseConnection` directly — `sea_orm::DatabaseConnection` is
/// already Arc-backed internally, so no additional `Arc` wrapper is needed.
///
/// Handlers in protected routes extract this instead of going through
/// `State<SharedState>`, ensuring each request always uses the correct
/// profile database regardless of the current `AppState.active_profile`.
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
        parts.extensions.get::<ProfileDb>().cloned().ok_or_else(|| {
            AppError::InternalError("ProfileDb not found in request extensions".into())
                .into_response()
        })
    }
}

/// Middleware: inject `ProfileDb` into request extensions based on session profile.
///
/// Must be placed AFTER `AuthManagerLayer` in the layer stack (innermost) so that
/// the auth session is already populated when this runs.
///
/// - Authenticated request: reads `(mode, _)` from `auth_session.user.id()`
///   and inserts the corresponding database.
/// - Unauthenticated request: falls back to `state.active_profile`.
pub async fn profile_db_middleware(
    State(state): State<SharedState>,
    auth_session: AuthSession<Backend>,
    mut request: Request,
    next: Next,
) -> Response {
    let db = if let Some(user) = &auth_session.user {
        let ProfileUserId(mode, _) = user.id();
        state.db_for(mode).clone()
    } else {
        state.db_for(*state.active_profile.read()).clone()
    };

    request.extensions_mut().insert(ProfileDb(db));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    // ProfileDb is a thin wrapper. Verify it clones correctly.
    // Full integration coverage is in profile_middleware.feature.

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

    /// Verify that from_request_parts returns the exact ProfileDb that was inserted,
    /// and that two distinct databases inserted for demo vs production sessions are
    /// correctly routed — the extracted handle queries the intended database.
    #[tokio::test]
    async fn routing_returns_correct_db_per_profile() {
        use axum::http::Request;
        use mokumo_db::DatabaseConnection;

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

        let demo_db = mokumo_db::initialize_database("sqlite::memory:?mode=rwc")
            .await
            .unwrap();
        let prod_db = mokumo_db::initialize_database("sqlite::memory:?mode=rwc")
            .await
            .unwrap();

        // Brand each DB with a distinct user_version so queries can tell them apart
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
