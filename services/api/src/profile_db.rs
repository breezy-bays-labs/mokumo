//! Profile-routing middleware for per-request DB selection.
//!
//! The `ProfileDb` type and its `FromRequestParts` impl live in `kikan`
//! (`kikan::profile_db`) so any vertical handler can extract it without
//! reaching into `services/api`. This module only carries the middleware,
//! which is welded to mokumo's `SharedState` shape (reads
//! `state.db_for(mode)` and `state.active_profile`) and so cannot move
//! into platform code without dragging AppState along.
//!
//! `profile_db_middleware` runs immediately after `AuthManagerLayer`. For
//! authenticated requests it reads the profile discriminant from the
//! compound user ID `(SetupMode, i64)` and inserts `ProfileDb` into
//! request extensions. For unauthenticated requests it falls back to
//! `AppState.active_profile`.
//!
//! Protected handlers extract the handle via `ProfileDb(db): kikan::ProfileDb`.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum_login::AuthSession;
use axum_login::AuthUser;

use crate::SharedState;
use kikan::ProfileDb;
use kikan::auth::Backend;
use kikan::auth::ProfileUserId;

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
