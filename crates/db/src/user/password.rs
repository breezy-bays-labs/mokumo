use mokumo_core::error::DomainError;

pub async fn hash_password(password: String) -> Result<String, DomainError> {
    tokio::task::spawn_blocking(move || password_auth::generate_hash(password))
        .await
        .map_err(|e| DomainError::Internal {
            message: format!("password hash task failed: {e}"),
        })
}

pub async fn verify_password(password: String, hash: String) -> Result<bool, DomainError> {
    tokio::task::spawn_blocking(
        move || match password_auth::verify_password(password, &hash) {
            Ok(()) => Ok(true),
            Err(password_auth::VerifyError::PasswordInvalid) => Ok(false),
            Err(e) => Err(DomainError::Internal {
                message: format!("password verification failed: {e}"),
            }),
        },
    )
    .await
    .map_err(|e| DomainError::Internal {
        message: format!("password verify task failed: {e}"),
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_password_produces_non_empty_hash() {
        let hash = hash_password("correct-horse-battery".to_string())
            .await
            .unwrap();
        assert!(!hash.is_empty(), "hash should not be empty");
        // PHC string format: starts with "$argon2" or "$bcrypt", etc.
        assert!(hash.starts_with('$'), "hash should be a PHC-format string");
    }

    #[tokio::test]
    async fn verify_password_returns_true_for_correct_password() {
        let hash = hash_password("correct-horse-battery".to_string())
            .await
            .unwrap();
        let valid = verify_password("correct-horse-battery".to_string(), hash)
            .await
            .unwrap();
        assert!(valid, "correct password should verify as true");
    }

    #[tokio::test]
    async fn verify_password_returns_false_for_wrong_password() {
        let hash = hash_password("correct-horse-battery".to_string())
            .await
            .unwrap();
        let valid = verify_password("wrong-password".to_string(), hash)
            .await
            .unwrap();
        assert!(!valid, "wrong password should verify as false");
    }

    #[tokio::test]
    async fn verify_password_returns_err_for_malformed_hash() {
        let result =
            verify_password("any-password".to_string(), "not-a-phc-hash".to_string()).await;
        assert!(
            result.is_err(),
            "malformed hash should return DomainError::Internal"
        );
    }
}
