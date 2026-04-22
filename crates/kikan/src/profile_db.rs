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

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::marker::PhantomData;
use std::str::FromStr;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{StatusCode, request::Parts};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_login::{AuthSession, AuthUser};

use crate::auth::{Backend, ProfileUserId};
use crate::db::DatabaseConnection;
use crate::platform_state::PlatformState;

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

/// Per-request view of the request's effective profile kind.
///
/// Generic over the vertical's profile discriminant `K` (e.g. Mokumo's
/// `SetupMode`). Inserted into request extensions by the same middleware
/// that provides `ProfileDb`. Handlers with profile-gated policy (e.g.
/// shop-specific endpoints that require a particular kind) extract this
/// instead of reaching into a shared `AppState`.
#[derive(Clone, Copy, Debug)]
pub struct ActiveProfile<K>(pub K);

impl<S, K> FromRequestParts<S> for ActiveProfile<K>
where
    S: Send + Sync,
    K: Clone + Send + Sync + 'static,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ActiveProfile<K>>()
            .cloned()
            .ok_or_else(missing_extension_response)
    }
}

/// Marker parameter for [`profile_db_middleware`] — callers pick `K` at
/// the mount site by spelling the turbofish.
#[derive(Clone, Copy)]
pub struct ProfileKindMarker<K>(PhantomData<fn() -> K>);

/// Profile-routing middleware: inject `ProfileDb` + `ActiveProfile<K>`
/// into request extensions based on session profile.
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
/// `from_fn_with_state(state.platform_state(), profile_db_middleware::<K>)`.
pub async fn profile_db_middleware<K>(
    State(platform): State<PlatformState>,
    auth_session: AuthSession<Backend<K>>,
    mut request: Request,
    next: Next,
) -> Response
where
    K: Copy
        + Debug
        + Display
        + Eq
        + Hash
        + Send
        + Sync
        + 'static
        + FromStr<Err = String>
        + serde::Serialize
        + serde::de::DeserializeOwned,
{
    let (mode, db) = if let Some(user) = &auth_session.user {
        let ProfileUserId(m, _) = user.id();
        let pool = auth_session
            .backend
            .db_for(&m)
            .cloned()
            .expect("authenticated session references a profile without a pool");
        (m, pool)
    } else {
        let active = platform.active_profile.read().clone();
        let m = match K::from_str(active.as_str()) {
            Ok(kind) => kind,
            Err(_) => {
                tracing::error!(
                    dir = active.as_str(),
                    "profile_db_middleware: active profile dir does not parse to ProfileKind"
                );
                return missing_extension_response();
            }
        };
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
/// wire shape (`{"code":"internal_error","message":...,"details":null}`).
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
#[path = "profile_db_tests.rs"]
mod tests;
