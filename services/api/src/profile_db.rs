//! Per-request database handle, selected by session profile.
//!
//! `ProfileDbMiddleware` runs immediately after `AuthManagerLayer`. For
//! authenticated requests it reads the profile discriminant from the compound
//! user ID `(SetupMode, i64)` and inserts `ProfileDb` into request extensions.
//! For unauthenticated requests it falls back to `AppState.active_profile`.
//!
//! Protected handlers extract the handle via `ProfileDb(db): ProfileDb`.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use axum_login::AuthSession;
use axum_login::AuthUser;
use mokumo_db::DatabaseConnection;

use crate::SharedState;
use crate::auth::backend::Backend;
use crate::auth::user::ProfileUserId;

/// Per-request database handle injected by `ProfileDbMiddleware`.
///
/// Handlers in protected routes extract this instead of going through
/// `State<SharedState>`, ensuring each request always uses the correct
/// profile database regardless of the current `AppState.active_profile`.
#[derive(Clone, Debug)]
pub struct ProfileDb(pub Arc<DatabaseConnection>);

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
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<ProfileDb>().cloned().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "ProfileDb not found in request extensions — ensure ProfileDbMiddleware is wired",
        ))
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
        state.db_for(state.active_profile).clone()
    };

    request.extensions_mut().insert(ProfileDb(Arc::new(db)));
    next.run(request).await
}

#[cfg(test)]
mod tests {
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
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
