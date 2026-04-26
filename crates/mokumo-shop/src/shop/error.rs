//! Shop-logo handler error type.
//!
//! Mirrors `CustomerHandlerError` — wraps `DomainError` plus a small set of
//! vertical-specific variants (format/size/dimension rejection, production
//! profile guard, rate-limit) and maps each into a wire-identical JSON error
//! envelope so existing Hurl tests keep passing byte-for-byte.
//!
//! Post-S4.3: platform-owned codes (`Unauthorized`, `RateLimited`,
//! `ValidationError`, etc.) stay typed as `kikan_types::error::ErrorCode` and
//! flow through `ErrorBody`. Shop-vertical codes
//! (`ShopLogoRequiresProductionProfile`, `Logo*`, `MissingField`,
//! `ShopLogoNotFound`) are typed as `mokumo_shop::types::error::ShopErrorCode`
//! and flow through `ShopErrorBody`. Both serialize to the same
//! `{"code": "...", "message": "...", "details": null}` wire shape.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kikan::error::DomainError;
use kikan_types::error::{ErrorBody, ErrorCode};

use crate::shop::logo_validator::LogoError;
use crate::types::error::{ShopErrorBody, ShopErrorCode};

#[derive(Debug)]
pub enum ShopLogoHandlerError {
    Domain(DomainError),
    /// 403 with a shop-vertical code (e.g. `ShopLogoRequiresProductionProfile`).
    ShopForbidden {
        code: ShopErrorCode,
        message: String,
    },
    /// 401 with a platform-owned code (typically `Unauthorized`).
    Unauthorized {
        code: ErrorCode,
        message: String,
    },
    TooManyRequests(String),
    /// 400 with a shop-vertical code (e.g. `MissingField`).
    ShopBadRequest {
        code: ShopErrorCode,
        message: String,
    },
    /// 422 with a shop-vertical code (e.g. `LogoTooLarge`, `LogoMalformed`).
    ShopUnprocessable {
        code: ShopErrorCode,
        message: String,
    },
    Internal(String),
}

impl From<DomainError> for ShopLogoHandlerError {
    fn from(err: DomainError) -> Self {
        Self::Domain(err)
    }
}

impl From<LogoError> for ShopLogoHandlerError {
    fn from(err: LogoError) -> Self {
        match err {
            LogoError::FormatUnsupported { .. } => Self::ShopUnprocessable {
                code: ShopErrorCode::LogoFormatUnsupported,
                message: "Only PNG, JPEG, or WebP files are accepted.".into(),
            },
            LogoError::TooLarge => Self::ShopUnprocessable {
                code: ShopErrorCode::LogoTooLarge,
                message: "File is too large. Max 2 MB.".into(),
            },
            LogoError::DimensionsExceeded => Self::ShopUnprocessable {
                code: ShopErrorCode::LogoDimensionsExceeded,
                message: "Image is too large. Max 2048×2048 pixels.".into(),
            },
            LogoError::Malformed => Self::ShopUnprocessable {
                code: ShopErrorCode::LogoMalformed,
                message: "File unreadable. Try another image.".into(),
            },
        }
    }
}

impl IntoResponse for ShopLogoHandlerError {
    fn into_response(self) -> Response {
        let mut response = match self {
            Self::Domain(DomainError::NotFound { entity, id }) => platform_body(
                StatusCode::NOT_FOUND,
                ErrorBody {
                    code: ErrorCode::NotFound,
                    message: format!("{entity} with id {id} not found"),
                    details: None,
                },
            ),
            Self::Domain(DomainError::Conflict { message }) => platform_body(
                StatusCode::CONFLICT,
                ErrorBody {
                    code: ErrorCode::Conflict,
                    message,
                    details: None,
                },
            ),
            Self::Domain(DomainError::Validation { details }) => platform_body(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorBody {
                    code: ErrorCode::ValidationError,
                    message: "Validation failed".into(),
                    details: Some(details),
                },
            ),
            Self::Domain(DomainError::Internal { message }) => {
                tracing::error!("Shop logo internal error: {message}");
                platform_body(StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
            }
            #[allow(unreachable_patterns)]
            Self::Domain(other) => {
                tracing::error!("Unhandled domain error: {other}");
                platform_body(StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
            }
            Self::ShopForbidden { code, message } => shop_body(
                StatusCode::FORBIDDEN,
                ShopErrorBody {
                    code,
                    message,
                    details: None,
                },
            ),
            Self::Unauthorized { code, message } => platform_body(
                StatusCode::UNAUTHORIZED,
                ErrorBody {
                    code,
                    message,
                    details: None,
                },
            ),
            Self::TooManyRequests(message) => platform_body(
                StatusCode::TOO_MANY_REQUESTS,
                ErrorBody {
                    code: ErrorCode::RateLimited,
                    message,
                    details: None,
                },
            ),
            Self::ShopBadRequest { code, message } => shop_body(
                StatusCode::BAD_REQUEST,
                ShopErrorBody {
                    code,
                    message,
                    details: None,
                },
            ),
            Self::ShopUnprocessable { code, message } => shop_body(
                StatusCode::UNPROCESSABLE_ENTITY,
                ShopErrorBody {
                    code,
                    message,
                    details: None,
                },
            ),
            Self::Internal(message) => {
                tracing::error!("Shop logo internal: {message}");
                platform_body(StatusCode::INTERNAL_SERVER_ERROR, redacted_internal())
            }
        };

        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            "no-store".parse().expect("static header value parses"),
        );
        response
    }
}

fn platform_body(status: StatusCode, body: ErrorBody) -> Response {
    (status, Json(body)).into_response()
}

fn shop_body(status: StatusCode, body: ShopErrorBody) -> Response {
    (status, Json(body)).into_response()
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

    #[test]
    fn not_found_maps_to_404() {
        let err = ShopLogoHandlerError::from(DomainError::NotFound {
            entity: "shop_logo",
            id: "1".into(),
        });
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn logo_too_large_maps_to_422() {
        let err = ShopLogoHandlerError::from(LogoError::TooLarge);
        assert_eq!(
            err.into_response().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn forbidden_maps_to_403_with_cache_no_store() {
        let err = ShopLogoHandlerError::ShopForbidden {
            code: ShopErrorCode::ShopLogoRequiresProductionProfile,
            message: "nope".into(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .unwrap(),
            "no-store"
        );
    }

    #[test]
    fn too_many_requests_maps_to_429() {
        let err = ShopLogoHandlerError::TooManyRequests("slow down".into());
        assert_eq!(err.into_response().status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn internal_redacts_message() {
        let err = ShopLogoHandlerError::Internal("secret reason".into());
        let body = axum::body::to_bytes(err.into_response().into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert!(!parsed.message.contains("secret"));
    }

    #[tokio::test]
    async fn shop_forbidden_wire_shape_preserves_code_string() {
        // Byte-for-byte wire preservation: the shop-vertical code must still
        // serialize to the same snake_case string the pre-split ErrorCode used.
        let err = ShopLogoHandlerError::ShopForbidden {
            code: ShopErrorCode::ShopLogoRequiresProductionProfile,
            message: "nope".into(),
        };
        let body = axum::body::to_bytes(err.into_response().into_body(), usize::MAX)
            .await
            .unwrap();
        let json = std::str::from_utf8(&body).unwrap();
        assert!(
            json.contains("\"code\":\"shop_logo_requires_production_profile\""),
            "unexpected wire shape: {json}"
        );
    }

    #[tokio::test]
    async fn shop_unprocessable_logo_too_large_wire_shape() {
        let err = ShopLogoHandlerError::from(LogoError::TooLarge);
        let body = axum::body::to_bytes(err.into_response().into_body(), usize::MAX)
            .await
            .unwrap();
        let json = std::str::from_utf8(&body).unwrap();
        assert!(
            json.contains("\"code\":\"logo_too_large\""),
            "unexpected wire shape: {json}"
        );
    }
}
