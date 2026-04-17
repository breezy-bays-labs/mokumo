//! Customer handler error type.
//!
//! Maps `DomainError` into HTTP responses matching the platform-wide
//! `ErrorBody` wire shape byte-for-byte with `AppError::Domain` arms in
//! `services/api/src/error.rs`. Keeping the shape identical preserves
//! every existing Hurl test and frontend contract during the V6c
//! vertical-extraction.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kikan_types::error::{ErrorBody, ErrorCode};
use mokumo_core::error::DomainError;

/// Error returned from customer handlers.
///
/// Thin wrapper around `DomainError` so `mokumo-shop` can `IntoResponse`
/// without depending on `services/api`'s `AppError`.
#[derive(Debug)]
pub enum CustomerHandlerError {
    Domain(DomainError),
}

impl From<DomainError> for CustomerHandlerError {
    fn from(err: DomainError) -> Self {
        Self::Domain(err)
    }
}

impl IntoResponse for CustomerHandlerError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Domain(DomainError::NotFound { entity, id }) => (
                StatusCode::NOT_FOUND,
                ErrorBody {
                    code: ErrorCode::NotFound,
                    message: format!("{entity} with id {id} not found"),
                    details: None,
                },
            ),
            Self::Domain(DomainError::Conflict { message }) => (
                StatusCode::CONFLICT,
                ErrorBody {
                    code: ErrorCode::Conflict,
                    message,
                    details: None,
                },
            ),
            Self::Domain(DomainError::Validation { details }) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorBody {
                    code: ErrorCode::ValidationError,
                    message: "Validation failed".into(),
                    details: Some(details),
                },
            ),
            Self::Domain(DomainError::Internal { message }) => {
                tracing::error!("Internal error: {message}");
                (StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
            }
            // Rust 2024 non-exhaustive safeguard — future DomainError variants
            // fall through to a redacted 500 rather than failing compilation.
            #[allow(unreachable_patterns)]
            Self::Domain(other) => {
                tracing::error!("Unhandled domain error: {other}");
                (StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
            }
        };

        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            "no-store".parse().expect("static header value parses"),
        );
        response
    }
}

fn redacted_internal() -> ErrorBody {
    ErrorBody {
        code: ErrorCode::InternalError,
        message: "An internal error occurred".into(),
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn not_found_maps_to_404() {
        let err = CustomerHandlerError::from(DomainError::NotFound {
            entity: "customer",
            id: "1".into(),
        });
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn conflict_maps_to_409() {
        let err = CustomerHandlerError::from(DomainError::Conflict {
            message: "dup".into(),
        });
        assert_eq!(err.into_response().status(), StatusCode::CONFLICT);
    }

    #[test]
    fn validation_maps_to_422() {
        let err = CustomerHandlerError::from(DomainError::Validation {
            details: HashMap::new(),
        });
        assert_eq!(
            err.into_response().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn internal_maps_to_500_and_has_cache_no_store() {
        let err = CustomerHandlerError::from(DomainError::Internal {
            message: "boom".into(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .unwrap(),
            "no-store"
        );
    }

    #[tokio::test]
    async fn validation_response_includes_details() {
        let mut details = HashMap::new();
        details.insert("email".into(), vec!["invalid".into()]);

        let err = CustomerHandlerError::from(DomainError::Validation { details });
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.code, ErrorCode::ValidationError);
        assert_eq!(parsed.details.unwrap()["email"], vec!["invalid"]);
    }

    #[tokio::test]
    async fn internal_redacts_message() {
        let err = CustomerHandlerError::from(DomainError::Internal {
            message: "secret connection string".into(),
        });
        let body = axum::body::to_bytes(err.into_response().into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.code, ErrorCode::InternalError);
        assert!(!parsed.message.contains("secret"));
    }
}
