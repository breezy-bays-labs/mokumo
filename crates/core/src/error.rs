use std::collections::HashMap;

/// Domain-level errors representing business rule violations.
///
/// These are framework-agnostic — no HTTP, no Axum. The API layer
/// converts them into AppError with appropriate status codes.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("{entity} with id {id} not found")]
    NotFound { entity: &'static str, id: String },

    #[error("{message}")]
    Conflict { message: String },

    #[error("Validation failed")]
    Validation {
        details: HashMap<String, Vec<String>>,
    },

    #[error("{message}")]
    Internal { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_display_includes_entity_and_id() {
        let err = DomainError::NotFound {
            entity: "customer",
            id: "42".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("customer") && msg.contains("42"),
            "Expected display to contain entity and id, got: {msg}"
        );
    }

    #[test]
    fn conflict_display_includes_message() {
        let err = DomainError::Conflict {
            message: "email already exists".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("email already exists"),
            "Expected display to contain message, got: {msg}"
        );
    }

    #[test]
    fn validation_display_is_descriptive() {
        let mut details = HashMap::new();
        details.insert("email".into(), vec!["invalid format".into()]);
        let err = DomainError::Validation { details };
        let msg = err.to_string();
        // Should mention validation, not just "todo"
        assert!(
            msg.to_lowercase().contains("validation"),
            "Expected display to mention validation, got: {msg}"
        );
    }

    #[test]
    fn internal_display_includes_message() {
        let err = DomainError::Internal {
            message: "disk full".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("disk full"),
            "Expected display to contain message, got: {msg}"
        );
    }
}
