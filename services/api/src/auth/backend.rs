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
/// `authenticate` uses the `active_profile` database (login form is only
/// reachable in production mode; demo users auto-login via middleware).
///
/// `get_user` dispatches to the correct database by the profile discriminant
/// in the compound user ID `(SetupMode, i64)`.
#[derive(Clone)]
pub struct Backend {
    pub demo_db: DatabaseConnection,
    pub production_db: DatabaseConnection,
    /// The profile active at the time this Backend instance was created.
    /// Used by `authenticate` to select which DB to check credentials against.
    pub active_profile: SetupMode,
}

impl Backend {
    pub fn new(
        demo_db: DatabaseConnection,
        production_db: DatabaseConnection,
        active_profile: SetupMode,
    ) -> Self {
        Self {
            demo_db,
            production_db,
            active_profile,
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
        let db = self.db_for(self.active_profile);
        let repo = SeaOrmUserRepo::new(db.clone());
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
                self.active_profile,
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
}
