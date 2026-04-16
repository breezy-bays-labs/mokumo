use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mokumo_core::error::DomainError;
use mokumo_types::error::{ErrorBody, ErrorCode};

/// Application-level error that converts domain errors into HTTP responses.
///
/// This is the boundary between domain logic and HTTP semantics.
/// Internal errors are redacted — real messages are logged, not returned.
#[derive(Debug)]
pub enum AppError {
    Domain(DomainError),
    Database(sqlx::Error),
    /// 401 — not authenticated or invalid credentials.
    /// The `ErrorCode` distinguishes `Unauthorized` from `InvalidCredentials`.
    Unauthorized(ErrorCode, String),
    /// 403 — action not allowed (e.g. setup already completed).
    /// The `ErrorCode` distinguishes the specific forbidden reason.
    Forbidden(ErrorCode, String),
    /// 422 — unprocessable entity (e.g. logo validation failures).
    UnprocessableEntity(ErrorCode, String),
    /// 400 — bad request with a specific error code (e.g. validation in recovery flows).
    BadRequest(ErrorCode, String),
    /// 429 — rate limit exceeded.
    TooManyRequests(String),
    /// 503 — service unavailable (e.g. demo admin not found).
    ServiceUnavailable(String),
    /// 409 — state conflict with a specific restore error code.
    /// Named `StateConflict` (not `Conflict`) to avoid shadowing
    /// `DomainError::Conflict` which always maps to `ErrorCode::Conflict`.
    /// Used by restore endpoints for `ProductionDbExists` and `RestoreInProgress`.
    StateConflict(ErrorCode, String),
    /// 500 — generic internal error. The real message is logged, not returned.
    InternalError(String),
    /// 423 — demo installation is incomplete or corrupted (admin account missing,
    /// inactive, soft-deleted, or has no password hash).
    DemoSetupRequired,
    /// 423 — account locked after too many consecutive failed login attempts.
    AccountLocked(String),
}

impl From<DomainError> for AppError {
    fn from(err: DomainError) -> Self {
        Self::Domain(err)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Domain(domain_err) => match domain_err {
                DomainError::NotFound { entity, id } => (
                    StatusCode::NOT_FOUND,
                    ErrorBody {
                        code: ErrorCode::NotFound,
                        message: format!("{entity} with id {id} not found"),
                        details: None,
                    },
                ),
                DomainError::Conflict { message } => (
                    StatusCode::CONFLICT,
                    ErrorBody {
                        code: ErrorCode::Conflict,
                        message,
                        details: None,
                    },
                ),
                DomainError::Validation { details } => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorBody {
                        code: ErrorCode::ValidationError,
                        message: "Validation failed".into(),
                        details: Some(details),
                    },
                ),
                DomainError::Internal { message } => {
                    tracing::error!("Internal error: {message}");
                    (StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
                }
                // Required: Rust 2024 edition treats external enums as non-exhaustive,
                // so new DomainError variants added in crates/core/ won't break compilation here.
                #[allow(unreachable_patterns)]
                other => {
                    tracing::error!("Unhandled domain error: {other}");
                    (StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
                }
            },
            Self::Unauthorized(code, msg) => (
                StatusCode::UNAUTHORIZED,
                ErrorBody {
                    code,
                    message: msg,
                    details: None,
                },
            ),
            Self::Forbidden(code, msg) => (
                StatusCode::FORBIDDEN,
                ErrorBody {
                    code,
                    message: msg,
                    details: None,
                },
            ),
            Self::UnprocessableEntity(code, msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorBody {
                    code,
                    message: msg,
                    details: None,
                },
            ),
            Self::BadRequest(code, msg) => (
                StatusCode::BAD_REQUEST,
                ErrorBody {
                    code,
                    message: msg,
                    details: None,
                },
            ),
            Self::TooManyRequests(msg) => (
                StatusCode::TOO_MANY_REQUESTS,
                ErrorBody {
                    code: ErrorCode::RateLimited,
                    message: msg,
                    details: None,
                },
            ),
            Self::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorBody {
                    code: ErrorCode::InternalError,
                    message: msg,
                    details: None,
                },
            ),
            Self::StateConflict(code, msg) => (
                StatusCode::CONFLICT,
                ErrorBody {
                    code,
                    message: msg,
                    details: None,
                },
            ),
            Self::InternalError(msg) => {
                tracing::error!("Internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
            }
            Self::DemoSetupRequired => (
                StatusCode::LOCKED,
                ErrorBody {
                    code: ErrorCode::DemoSetupRequired,
                    message: "Demo installation is incomplete or corrupted. Reset demo data to restore access.".into(),
                    details: None,
                },
            ),
            Self::AccountLocked(msg) => (
                StatusCode::LOCKED,
                ErrorBody {
                    code: ErrorCode::AccountLocked,
                    message: msg,
                    details: None,
                },
            ),
            // Boundary safeguard: repo impls currently normalise DB errors into
            // DomainError before they reach here, so this arm fires only for raw
            // SQLx queries (e.g. future reporting endpoints) that bypass the repo layer.
            Self::Database(err) => match &err {
                sqlx::Error::RowNotFound => (
                    StatusCode::NOT_FOUND,
                    ErrorBody {
                        code: ErrorCode::NotFound,
                        message: "The requested record was not found".into(),
                        details: None,
                    },
                ),
                other => {
                    tracing::error!("Database error: {other}");
                    (StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
                }
            },
        };

        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            "no-store".parse().unwrap(),
        );
        response
    }
}

pub(crate) fn redacted_internal() -> ErrorBody {
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

    // --- Status code mapping ---

    #[test]
    fn not_found_maps_to_404() {
        let err = AppError::from(DomainError::NotFound {
            entity: "customer",
            id: "1".into(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn conflict_maps_to_409() {
        let err = AppError::from(DomainError::Conflict {
            message: "duplicate".into(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn validation_maps_to_422() {
        let err = AppError::from(DomainError::Validation {
            details: HashMap::new(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn internal_maps_to_500() {
        let err = AppError::from(DomainError::Internal {
            message: "oops".into(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // --- Response body shape ---

    #[tokio::test]
    async fn not_found_response_has_error_body_shape() {
        let err = AppError::from(DomainError::NotFound {
            entity: "customer",
            id: "42".into(),
        });
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::NotFound);
        assert!(error_body.message.contains("customer"));
        assert!(error_body.details.is_none());
    }

    #[tokio::test]
    async fn validation_response_includes_details() {
        let mut details = HashMap::new();
        details.insert("email".into(), vec!["invalid".into()]);

        let err = AppError::from(DomainError::Validation { details });
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::ValidationError);
        let d = error_body.details.unwrap();
        assert_eq!(d["email"], vec!["invalid"]);
    }

    #[tokio::test]
    async fn conflict_response_body() {
        let err = AppError::from(DomainError::Conflict {
            message: "email already exists".into(),
        });
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::Conflict);
        assert!(error_body.message.contains("email already exists"));
    }

    // --- Internal error redaction (security-critical) ---

    #[tokio::test]
    async fn internal_error_redacts_real_message() {
        let err = AppError::from(DomainError::Internal {
            message: "secret database connection string exposed".into(),
        });
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::InternalError);
        // MUST NOT contain the real error message
        assert!(
            !error_body.message.contains("secret"),
            "Internal error message was leaked: {}",
            error_body.message
        );
        assert!(
            !error_body.message.contains("database"),
            "Internal error details were leaked: {}",
            error_body.message
        );
    }

    #[tokio::test]
    async fn sqlx_error_redacts_details() {
        let err = AppError::Database(sqlx::Error::ColumnNotFound("secret_column".into()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::InternalError);
        assert!(
            !error_body.message.contains("secret_column"),
            "Database column name leaked: {}",
            error_body.message
        );
        assert!(
            !error_body.message.contains("ColumnNotFound"),
            "SQLx error variant leaked: {}",
            error_body.message
        );
    }

    // --- Cache-Control header ---

    #[test]
    fn error_response_has_cache_control_no_store() {
        let err = AppError::from(DomainError::NotFound {
            entity: "customer",
            id: "1".into(),
        });
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }

    #[test]
    fn internal_error_has_cache_control_no_store() {
        let err = AppError::from(DomainError::Internal {
            message: "test".into(),
        });
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }

    // --- Content-Type header ---

    #[test]
    fn response_has_json_content_type() {
        let err = AppError::from(DomainError::NotFound {
            entity: "customer",
            id: "1".into(),
        });
        let response = err.into_response();
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .expect("Missing Content-Type header");
        assert!(
            content_type.to_str().unwrap().contains("application/json"),
            "Expected JSON content type, got: {:?}",
            content_type
        );
    }

    // --- sqlx::Error categorization (#38) ---

    #[test]
    fn sqlx_row_not_found_maps_to_404() {
        let err = AppError::Database(sqlx::Error::RowNotFound);
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn sqlx_row_not_found_response_body() {
        let err = AppError::Database(sqlx::Error::RowNotFound);
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::NotFound);
        assert!(
            !error_body.message.is_empty(),
            "RowNotFound should have a user-facing message"
        );
    }

    #[tokio::test]
    async fn sqlx_other_errors_still_map_to_500_and_redact() {
        let err = AppError::Database(sqlx::Error::PoolTimedOut);
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::InternalError);
        assert!(
            !error_body.message.contains("PoolTimedOut"),
            "sqlx error variant should not leak: {}",
            error_body.message
        );
    }

    // --- HTTP-semantic variants (#248) ---

    #[test]
    fn unauthorized_maps_to_401() {
        let err = AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unauthorized_preserves_error_code() {
        let err = AppError::Unauthorized(
            ErrorCode::InvalidCredentials,
            "Invalid email or password".into(),
        );
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::InvalidCredentials);
        assert!(error_body.message.contains("Invalid email"));
    }

    #[test]
    fn forbidden_maps_to_403() {
        let err = AppError::Forbidden(ErrorCode::Forbidden, "Setup already completed".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn forbidden_response_body() {
        let err = AppError::Forbidden(ErrorCode::Forbidden, "Setup already completed".into());
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::Forbidden);
        assert!(error_body.message.contains("Setup already completed"));
    }

    #[test]
    fn bad_request_maps_to_400() {
        let err = AppError::BadRequest(ErrorCode::ValidationError, "Invalid PIN".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn bad_request_preserves_error_code() {
        let err = AppError::BadRequest(ErrorCode::ValidationError, "Invalid PIN".into());
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::ValidationError);
        assert!(error_body.message.contains("Invalid PIN"));
    }

    #[test]
    fn too_many_requests_maps_to_429() {
        let err = AppError::TooManyRequests("Try again later".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn too_many_requests_response_body() {
        let err = AppError::TooManyRequests("Try again later".into());
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::RateLimited);
    }

    #[test]
    fn service_unavailable_maps_to_503() {
        let err = AppError::ServiceUnavailable("Demo admin not found".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn state_conflict_maps_to_409() {
        let err = AppError::StateConflict(
            ErrorCode::ProductionDbExists,
            "Production database already exists".into(),
        );
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn state_conflict_preserves_error_code() {
        let err = AppError::StateConflict(
            ErrorCode::RestoreInProgress,
            "Another restore is already in progress".into(),
        );
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::RestoreInProgress);
        assert!(error_body.message.contains("Another restore"));
    }

    #[test]
    fn state_conflict_has_cache_control_no_store() {
        let err = AppError::StateConflict(
            ErrorCode::ProductionDbExists,
            "Production database already exists".into(),
        );
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }

    #[test]
    fn internal_error_variant_maps_to_500() {
        let err = AppError::InternalError("secret db string".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn internal_error_variant_redacts_message() {
        let err = AppError::InternalError("secret database connection string".into());
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::InternalError);
        assert!(
            !error_body.message.contains("secret"),
            "InternalError message was leaked: {}",
            error_body.message
        );
    }

    #[test]
    fn unauthorized_has_cache_control_no_store() {
        let err = AppError::Unauthorized(ErrorCode::Unauthorized, "test".into());
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }

    #[test]
    fn forbidden_has_cache_control_no_store() {
        let err = AppError::Forbidden(ErrorCode::Forbidden, "test".into());
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }

    #[test]
    fn demo_setup_required_maps_to_423() {
        let err = AppError::DemoSetupRequired;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::LOCKED);
    }

    #[tokio::test]
    async fn demo_setup_required_response_body() {
        let err = AppError::DemoSetupRequired;
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::DemoSetupRequired);
        assert!(error_body.message.contains("incomplete or corrupted"));
        assert!(error_body.details.is_none());
    }

    #[test]
    fn demo_setup_required_has_cache_control_no_store() {
        let err = AppError::DemoSetupRequired;
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }

    #[test]
    fn account_locked_maps_to_423() {
        let err = AppError::AccountLocked("Account locked for 15 minutes".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::LOCKED);
    }

    #[tokio::test]
    async fn account_locked_response_body() {
        let err = AppError::AccountLocked("Account locked for 15 minutes".into());
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::AccountLocked);
        assert!(error_body.message.contains("locked"));
        assert!(error_body.details.is_none());
    }

    #[test]
    fn account_locked_has_cache_control_no_store() {
        let err = AppError::AccountLocked("Account locked".into());
        let response = err.into_response();
        let cache_control = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .expect("Missing Cache-Control header");
        assert_eq!(cache_control.to_str().unwrap(), "no-store");
    }
}
