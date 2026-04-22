//! User administration HTTP handlers — thin delegations over the pure
//! `kikan::control_plane::users::*` layer.
//!
//! The same business-logic entry points are reachable from HTTP, UDS, and
//! in-process CLI subcommands without re-implementing the authorization +
//! last-admin guards.
//!
//! Handler bodies are thin: extract per-request state (session, DB),
//! call the pure fn, render the result (or map `ControlPlaneError →
//! AppError`). The `PermissionDenied` wire message ("Admin access
//! required") is preserved byte-for-byte by a local error mapper —
//! the pure-fn layer reports the semantic via `PermissionDenied`; the
//! HTTP adapter translates that back to the handler-specific wording
//! the client already knows.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{delete, patch};
use kikan_types::error::ErrorCode;
use kikan_types::user::{UpdateUserRoleRequest, UserResponse};

use kikan::auth::{RoleId, User, UserId};
use kikan::control_plane::{self, ControlPlaneState};
use kikan::{AppError, ControlPlaneError, ProfileDb};

use crate::auth_handlers::AuthSessionType;

pub fn user_admin_router() -> Router<ControlPlaneState> {
    Router::new()
        .route("/{id}", delete(soft_delete_user))
        .route("/{id}/role", patch(update_user_role))
}

fn to_response(u: User) -> UserResponse {
    UserResponse {
        id: u.id.get(),
        email: u.email,
        name: u.name,
        role_name: match u.role_id {
            RoleId::ADMIN => "Admin".into(),
            RoleId::STAFF => "Staff".into(),
            RoleId::GUEST => "Guest".into(),
            _ => "Unknown".into(),
        },
        is_active: u.is_active,
        last_login_at: u.last_login_at,
        created_at: u.created_at,
        updated_at: u.updated_at,
        deleted_at: u.deleted_at,
    }
}

/// Map a user-admin `ControlPlaneError` into the handler-specific
/// `AppError`. Preserves the legacy 403 wire message ("Admin access
/// required") that the pure-fn layer has no context to produce.
fn map_user_admin_error(err: ControlPlaneError) -> AppError {
    match err {
        ControlPlaneError::PermissionDenied => {
            AppError::Forbidden(ErrorCode::Forbidden, "Admin access required".into())
        }
        other => other.into(),
    }
}

async fn soft_delete_user(
    State(state): State<ControlPlaneState>,
    auth_session: AuthSessionType,
    ProfileDb(db): ProfileDb,
    Path(id): Path<i64>,
) -> Result<Json<UserResponse>, AppError> {
    let caller = auth_session.user.as_ref().ok_or_else(|| {
        AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into())
    })?;
    let user = control_plane::users::soft_delete_user(&state, &db, UserId::new(id), caller)
        .await
        .map_err(map_user_admin_error)?;
    Ok(Json(to_response(user)))
}

async fn update_user_role(
    State(state): State<ControlPlaneState>,
    auth_session: AuthSessionType,
    ProfileDb(db): ProfileDb,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRoleRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let caller = auth_session.user.as_ref().ok_or_else(|| {
        AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into())
    })?;
    let user = control_plane::users::update_user_role(
        &state,
        &db,
        UserId::new(id),
        RoleId::new(req.role_id),
        caller,
    )
    .await
    .map_err(map_user_admin_error)?;
    Ok(Json(to_response(user)))
}
