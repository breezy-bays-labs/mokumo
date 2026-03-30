use axum_login::AuthUser;
use mokumo_core::setup::SetupMode;
use mokumo_core::user::User;

/// Compound user identity: profile discriminant + database-level user ID.
///
/// Encodes which database the user belongs to so
/// `axum_login::AuthnBackend::get_user` can route the lookup to the correct
/// database without any separate session context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProfileUserId(pub SetupMode, pub i64);

impl std::fmt::Display for ProfileUserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use explicit literals to lock the session wire format independent of
        // SetupMode's Display impl. A change to SetupMode::Display must not
        // silently invalidate stored sessions.
        let profile = match self.0 {
            SetupMode::Demo => "demo",
            SetupMode::Production => "production",
        };
        write!(f, "{}:{}", profile, self.1)
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user: User,
    pub mode: SetupMode,
    password_hash_bytes: Vec<u8>,
}

impl AuthenticatedUser {
    pub fn new(user: User, password_hash: String, mode: SetupMode) -> Self {
        Self {
            user,
            mode,
            password_hash_bytes: password_hash.into_bytes(),
        }
    }
}

impl AuthUser for AuthenticatedUser {
    type Id = ProfileUserId;

    fn id(&self) -> Self::Id {
        ProfileUserId(self.mode, self.user.id.get())
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.password_hash_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_login::AuthUser;
    use mokumo_core::user::{RoleId, UserId};

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
            AuthenticatedUser::new(test_user(), "$argon2id$hash".into(), SetupMode::Demo);
        assert_eq!(auth_user.id(), ProfileUserId(SetupMode::Demo, 1_i64));
    }

    #[test]
    fn auth_user_id_production_mode() {
        let auth_user =
            AuthenticatedUser::new(test_user(), "$argon2id$hash".into(), SetupMode::Production);
        assert_eq!(auth_user.id(), ProfileUserId(SetupMode::Production, 1_i64));
    }

    /// Lock the session wire format: "demo:1" and "production:1".
    /// Any change to this output invalidates persisted sessions for live users.
    #[test]
    fn profile_user_id_display_format_is_locked() {
        assert_eq!(ProfileUserId(SetupMode::Demo, 1).to_string(), "demo:1",);
        assert_eq!(
            ProfileUserId(SetupMode::Production, 42).to_string(),
            "production:42",
        );
    }

    #[test]
    fn session_auth_hash_returns_password_bytes() {
        let hash = "$argon2id$v=19$m=19456,t=2,p=1$salt$hash";
        let auth_user = AuthenticatedUser::new(test_user(), hash.into(), SetupMode::Demo);
        assert_eq!(auth_user.session_auth_hash(), hash.as_bytes());
    }

    #[test]
    fn different_password_hash_produces_different_session_hash() {
        let user = test_user();
        let user1 = AuthenticatedUser::new(user.clone(), "hash_a".into(), SetupMode::Demo);
        let user2 = AuthenticatedUser::new(user, "hash_b".into(), SetupMode::Demo);
        assert_ne!(user1.session_auth_hash(), user2.session_auth_hash());
    }

    #[test]
    fn mode_is_accessible() {
        let demo = AuthenticatedUser::new(test_user(), "hash".into(), SetupMode::Demo);
        let prod = AuthenticatedUser::new(test_user(), "hash".into(), SetupMode::Production);
        assert_eq!(demo.mode, SetupMode::Demo);
        assert_eq!(prod.mode, SetupMode::Production);
    }
}
