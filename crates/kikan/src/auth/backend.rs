use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::Arc;

use axum_login::AuthnBackend;
use mokumo_core::error::DomainError;
use sea_orm::DatabaseConnection;

use super::domain::UserId;
use super::password;
use super::repo::SeaOrmUserRepo;
use super::user::{AuthenticatedUser, ProfileUserId};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Authentication backend holding every profile's database plus the kind
/// that credentialed login authenticates against.
///
/// `authenticate` always hits `pools[auth_kind]` — the vertical's
/// [`Graft::auth_profile_kind`] selects which pool. For Mokumo this is
/// `SetupMode::Production`: the setup wizard writes the admin account
/// there, and demo mode auto-logins without credentials.
///
/// `get_user` dispatches on the profile discriminant carried in
/// [`ProfileUserId`] so session lookups always see the database the user
/// was created in.
pub struct Backend<K> {
    pools: Arc<HashMap<K, DatabaseConnection>>,
    auth_kind: K,
}

impl<K: Clone> Clone for Backend<K> {
    fn clone(&self) -> Self {
        Self {
            pools: Arc::clone(&self.pools),
            auth_kind: self.auth_kind.clone(),
        }
    }
}

impl<K> Backend<K>
where
    K: Copy + Eq + Hash,
{
    pub fn new(pools: Arc<HashMap<K, DatabaseConnection>>, auth_kind: K) -> Self {
        Self { pools, auth_kind }
    }

    pub fn db_for(&self, kind: &K) -> Option<&DatabaseConnection> {
        self.pools.get(kind)
    }

    pub fn auth_kind(&self) -> K {
        self.auth_kind
    }
}

impl<K> AuthnBackend for Backend<K>
where
    K: Copy
        + Debug
        + Display
        + Eq
        + Hash
        + Send
        + Sync
        + 'static
        + serde::Serialize
        + serde::de::DeserializeOwned,
{
    type User = AuthenticatedUser<K>;
    type Credentials = Credentials;
    type Error = DomainError;

    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let auth_pool =
            self.pools
                .get(&self.auth_kind)
                .cloned()
                .ok_or_else(|| DomainError::Internal {
                    message: "Backend: auth_kind pool missing".into(),
                })?;
        let repo = SeaOrmUserRepo::new(auth_pool);
        let Some((user, hash)) = repo.find_by_email_with_hash(&creds.email).await? else {
            return Ok(None);
        };

        if !user.is_active {
            return Ok(None);
        }

        let is_valid = password::verify_password(creds.password, hash.clone()).await?;
        if is_valid {
            Ok(Some(AuthenticatedUser::new(user, hash, self.auth_kind)))
        } else {
            Ok(None)
        }
    }

    async fn get_user(
        &self,
        user_id: &ProfileUserId<K>,
    ) -> Result<Option<Self::User>, Self::Error> {
        let ProfileUserId(mode, raw_id) = *user_id;
        let db = self
            .pools
            .get(&mode)
            .cloned()
            .ok_or_else(|| DomainError::Internal {
                message: "Backend: session references profile without pool".into(),
            })?;
        let repo = SeaOrmUserRepo::new(db);
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
#[path = "backend_tests.rs"]
mod tests;
