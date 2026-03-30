use axum_login::AuthnBackend;
use mokumo_core::error::DomainError;
use mokumo_core::setup::SetupMode;
use mokumo_core::user::UserId;
use mokumo_db::DatabaseConnection;
use mokumo_db::user::password;
use mokumo_db::user::repo::SeaOrmUserRepo;

use super::user::{AuthenticatedUser, ProfileUserId};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Authentication backend holding both profile databases.
///
/// `authenticate` always checks `production_db` — the setup wizard writes the
/// admin account there, and demo mode auto-logins without credentials.
///
/// `get_user` dispatches to the correct database by the profile discriminant
/// in the compound user ID `(SetupMode, i64)`.
#[derive(Clone)]
pub struct Backend {
    pub demo_db: DatabaseConnection,
    pub production_db: DatabaseConnection,
}

impl Backend {
    pub fn new(demo_db: DatabaseConnection, production_db: DatabaseConnection) -> Self {
        Self {
            demo_db,
            production_db,
        }
    }

    fn db_for(&self, mode: SetupMode) -> &DatabaseConnection {
        match mode {
            SetupMode::Demo => &self.demo_db,
            SetupMode::Production => &self.production_db,
        }
    }
}

impl AuthnBackend for Backend {
    type User = AuthenticatedUser;
    type Credentials = Credentials;
    type Error = DomainError;

    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        // Credential-based login always authenticates against the production database.
        // The setup wizard always writes the admin account to production_db, and demo
        // mode auto-logins without credentials — so production_db is the only valid
        // target regardless of the current active_profile.
        let repo = SeaOrmUserRepo::new(self.production_db.clone());
        let Some((user, hash)) = repo.find_by_email_with_hash(&creds.email).await? else {
            return Ok(None);
        };

        if !user.is_active {
            return Ok(None);
        }

        let is_valid = password::verify_password(creds.password, hash.clone()).await?;
        if is_valid {
            Ok(Some(AuthenticatedUser::new(
                user,
                hash,
                SetupMode::Production,
            )))
        } else {
            Ok(None)
        }
    }

    async fn get_user(&self, user_id: &ProfileUserId) -> Result<Option<Self::User>, Self::Error> {
        let ProfileUserId(mode, raw_id) = *user_id;
        let db = self.db_for(mode);
        let repo = SeaOrmUserRepo::new(db.clone());
        let id = UserId::new(raw_id);
        let Some((user, hash)) = repo.find_by_id_with_hash(&id).await? else {
            return Ok(None);
        };
        if !user.is_active {
            return Ok(None);
        }
        Ok(Some(AuthenticatedUser::new(user, hash, mode)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Backend is constructed from two DatabaseConnections and a SetupMode.
    // Full integration tests for authenticate/get_user live in the BDD suite
    // (demo_auth.feature, session_invalidation.feature, profile_middleware.feature).
    // Here we just verify the constructor and db_for dispatch.

    // db_for tests require real DatabaseConnection — tested via integration tests.
    // This module primarily exercises the type-level guarantees.

    #[test]
    fn credentials_deserializes() {
        let json = r#"{"email":"a@b.com","password":"secret"}"#;
        let creds: Credentials = serde_json::from_str(json).unwrap();
        assert_eq!(creds.email, "a@b.com");
        assert_eq!(creds.password, "secret");
    }

    /// Lock the serde format of ProfileUserId so accidental format changes break CI.
    /// axum_login serialises this value into the session store — changing it
    /// invalidates all active sessions for live users.
    #[test]
    fn profile_user_id_roundtrip() {
        use crate::auth::user::ProfileUserId;

        let cases = [
            (ProfileUserId(SetupMode::Demo, 1), r#"["demo",1]"#),
            (
                ProfileUserId(SetupMode::Production, 99),
                r#"["production",99]"#,
            ),
        ];

        for (original, expected_json) in cases {
            let json = serde_json::to_string(&original).unwrap();
            assert_eq!(
                json, expected_json,
                "serialization format changed for {original:?}"
            );
            let restored: ProfileUserId = serde_json::from_str(expected_json).unwrap();
            assert_eq!(
                restored, original,
                "deserialization failed for {expected_json}"
            );
        }
    }
}
