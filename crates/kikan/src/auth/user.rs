use std::fmt::{Debug, Display};
use std::hash::Hash;

use axum_login::AuthUser;

use super::domain::User;

/// Compound user identity: profile discriminant + database-level user ID.
///
/// Encodes which database the user belongs to so
/// `axum_login::AuthnBackend::get_user` can route the lookup to the correct
/// database without any separate session context.
///
/// Generic over the vertical's profile kind — the opaque `K` flows through
/// `axum_login`'s `AuthUser::Id` associated type. Kikan never matches on
/// concrete variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProfileUserId<K>(pub K, pub i64);

impl<K: Display> Display for ProfileUserId<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser<K> {
    pub user: User,
    pub mode: K,
    password_hash_bytes: Vec<u8>,
}

impl<K> AuthenticatedUser<K> {
    pub fn new(user: User, password_hash: String, mode: K) -> Self {
        Self {
            user,
            mode,
            password_hash_bytes: password_hash.into_bytes(),
        }
    }
}

impl<K> AuthUser for AuthenticatedUser<K>
where
    K: Copy
        + Debug
        + Display
        + Hash
        + Eq
        + Send
        + Sync
        + 'static
        + serde::Serialize
        + serde::de::DeserializeOwned,
{
    type Id = ProfileUserId<K>;

    fn id(&self) -> Self::Id {
        ProfileUserId(self.mode, self.user.id.get())
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.password_hash_bytes
    }
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;
