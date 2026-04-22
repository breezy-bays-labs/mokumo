use super::*;
use crate::auth::{RoleId, UserId};
use axum_login::AuthUser;
use kikan_types::SetupMode;

fn test_user() -> User {
    User {
        id: UserId::new(1),
        email: "admin@shop.local".into(),
        name: "Admin".into(),
        role_id: RoleId::ADMIN,
        is_active: true,
        last_login_at: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        deleted_at: None,
    }
}

#[test]
fn auth_user_id_includes_mode_and_user_id() {
    let auth_user =
        AuthenticatedUser::<SetupMode>::new(test_user(), "$argon2id$hash".into(), SetupMode::Demo);
    assert_eq!(auth_user.id(), ProfileUserId(SetupMode::Demo, 1_i64));
}

#[test]
fn auth_user_id_production_mode() {
    let auth_user = AuthenticatedUser::<SetupMode>::new(
        test_user(),
        "$argon2id$hash".into(),
        SetupMode::Production,
    );
    assert_eq!(auth_user.id(), ProfileUserId(SetupMode::Production, 1_i64));
}

/// Lock the session wire format: "demo:1" and "production:1".
/// Any change to this output invalidates persisted sessions for live users.
/// The format comes from `K: Display` — `SetupMode::Display` emits `demo` /
/// `production` (kikan-types wire contract), so the generic `ProfileUserId<K>`
/// continues to render identically.
#[test]
fn profile_user_id_display_format_is_locked() {
    assert_eq!(
        ProfileUserId::<SetupMode>(SetupMode::Demo, 1).to_string(),
        "demo:1",
    );
    assert_eq!(
        ProfileUserId::<SetupMode>(SetupMode::Production, 42).to_string(),
        "production:42",
    );
}

#[test]
fn session_auth_hash_returns_password_bytes() {
    let hash = "$argon2id$v=19$m=19456,t=2,p=1$salt$hash";
    let auth_user = AuthenticatedUser::<SetupMode>::new(test_user(), hash.into(), SetupMode::Demo);
    assert_eq!(auth_user.session_auth_hash(), hash.as_bytes());
}

#[test]
fn different_password_hash_produces_different_session_hash() {
    let user = test_user();
    let user1 = AuthenticatedUser::<SetupMode>::new(user.clone(), "hash_a".into(), SetupMode::Demo);
    let user2 = AuthenticatedUser::<SetupMode>::new(user, "hash_b".into(), SetupMode::Demo);
    assert_ne!(user1.session_auth_hash(), user2.session_auth_hash());
}

#[test]
fn mode_is_accessible() {
    let demo = AuthenticatedUser::<SetupMode>::new(test_user(), "hash".into(), SetupMode::Demo);
    let prod =
        AuthenticatedUser::<SetupMode>::new(test_user(), "hash".into(), SetupMode::Production);
    assert_eq!(demo.mode, SetupMode::Demo);
    assert_eq!(prod.mode, SetupMode::Production);
}
