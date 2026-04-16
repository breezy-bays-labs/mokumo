use axum::extract::Path;
use axum::routing::{delete, patch};
use axum::{Json, Router};
use mokumo_core::user::service::UserService;
use mokumo_core::user::{RoleId, UserId};
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::error::ErrorCode;
use mokumo_types::user::{UpdateUserRoleRequest, UserResponse};

use crate::SharedState;
use crate::auth::AuthSessionType;
use crate::error::AppError;
use crate::profile_db::ProfileDb;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/{id}", delete(soft_delete_user))
        .route("/{id}/role", patch(update_user_role))
}

fn user_service(db: mokumo_db::DatabaseConnection) -> UserService<SeaOrmUserRepo> {
    UserService::new(SeaOrmUserRepo::new(db))
}

fn to_response(u: mokumo_core::user::User) -> UserResponse {
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

async fn soft_delete_user(
    auth_session: AuthSessionType,
    ProfileDb(db): ProfileDb,
    Path(id): Path<i64>,
) -> Result<Json<UserResponse>, AppError> {
    let caller = auth_session.user.as_ref().ok_or_else(|| {
        AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into())
    })?;
    if caller.user.role_id != RoleId::ADMIN {
        return Err(AppError::Forbidden(
            ErrorCode::Forbidden,
            "Admin access required".into(),
        ));
    }

    let actor_id = caller.user.id;
    let target_id = UserId::new(id);
    let svc = user_service(db);
    let user = svc.soft_delete_user(&target_id, actor_id).await?;
    Ok(Json(to_response(user)))
}

async fn update_user_role(
    auth_session: AuthSessionType,
    ProfileDb(db): ProfileDb,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRoleRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let caller = auth_session.user.as_ref().ok_or_else(|| {
        AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into())
    })?;
    if caller.user.role_id != RoleId::ADMIN {
        return Err(AppError::Forbidden(
            ErrorCode::Forbidden,
            "Admin access required".into(),
        ));
    }

    let actor_id = caller.user.id;
    let target_id = UserId::new(id);
    let new_role = RoleId::new(req.role_id);
    let svc = user_service(db);
    let user = svc.update_user_role(&target_id, new_role, actor_id).await?;
    Ok(Json(to_response(user)))
}
