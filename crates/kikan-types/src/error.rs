use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Machine-readable error code for API responses.
///
/// Serializes to snake_case strings (e.g. `NotFound` → `"not_found"`),
/// keeping the wire format unchanged from the previous `String` representation.
/// Both Rust and generated TypeScript get exhaustive matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    NotFound,
    Conflict,
    ValidationError,
    InternalError,
    /// Client-side only: response body was not parseable JSON.
    ParseError,
    /// Client-side only: network request failed (offline, DNS, etc.).
    NetworkError,
    /// Invalid credentials (wrong email or password).
    InvalidCredentials,
    /// Not authenticated (no valid session).
    Unauthorized,
    /// Action forbidden (e.g., setup already completed).
    Forbidden,
    /// Invalid setup token.
    InvalidToken,
    /// Setup failed (e.g., admin already exists).
    SetupFailed,
    /// Too many requests (rate limit exceeded).
    RateLimited,
    /// HTTP method not allowed on this endpoint.
    MethodNotAllowed,
    /// Production database file already exists; cannot restore over it.
    ProductionDbExists,
    /// File is not a valid Mokumo SQLite database (wrong application_id or not SQLite).
    NotMokumoDatabase,
    /// Database file failed integrity check (corrupted or truncated).
    DatabaseCorrupt,
    /// Database schema has migrations unknown to this binary (created by a newer version).
    SchemaIncompatible,
    /// A restore operation is already in progress.
    RestoreInProgress,
    /// Logo upload is only allowed on the production profile.
    ShopLogoRequiresProductionProfile,
    /// Uploaded file is not a PNG, JPEG, or WebP.
    LogoFormatUnsupported,
    /// Uploaded logo exceeds the 2 MiB size limit.
    LogoTooLarge,
    /// Uploaded logo dimensions exceed 2048×2048 pixels.
    LogoDimensionsExceeded,
    /// Uploaded logo file is malformed and cannot be read.
    LogoMalformed,
    /// Required multipart field is missing.
    MissingField,
    /// No shop logo has been uploaded.
    ShopLogoNotFound,
    /// Demo installation is incomplete — returned by `AppError::DemoSetupRequired` when
    /// `validate_installation()` determines that `admin@demo.local` is missing, inactive,
    /// soft-deleted, or has an empty `password_hash`.
    DemoSetupRequired,
    /// Account locked after too many consecutive failed login attempts.
    AccountLocked,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not_found"),
            Self::Conflict => write!(f, "conflict"),
            Self::ValidationError => write!(f, "validation_error"),
            Self::InternalError => write!(f, "internal_error"),
            Self::ParseError => write!(f, "parse_error"),
            Self::NetworkError => write!(f, "network_error"),
            Self::InvalidCredentials => write!(f, "invalid_credentials"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::Forbidden => write!(f, "forbidden"),
            Self::InvalidToken => write!(f, "invalid_token"),
            Self::SetupFailed => write!(f, "setup_failed"),
            Self::RateLimited => write!(f, "rate_limited"),
            Self::MethodNotAllowed => write!(f, "method_not_allowed"),
            Self::ProductionDbExists => write!(f, "production_db_exists"),
            Self::NotMokumoDatabase => write!(f, "not_mokumo_database"),
            Self::DatabaseCorrupt => write!(f, "database_corrupt"),
            Self::SchemaIncompatible => write!(f, "schema_incompatible"),
            Self::RestoreInProgress => write!(f, "restore_in_progress"),
            Self::ShopLogoRequiresProductionProfile => {
                write!(f, "shop_logo_requires_production_profile")
            }
            Self::LogoFormatUnsupported => write!(f, "logo_format_unsupported"),
            Self::LogoTooLarge => write!(f, "logo_too_large"),
            Self::LogoDimensionsExceeded => write!(f, "logo_dimensions_exceeded"),
            Self::LogoMalformed => write!(f, "logo_malformed"),
            Self::MissingField => write!(f, "missing_field"),
            Self::ShopLogoNotFound => write!(f, "shop_logo_not_found"),
            Self::DemoSetupRequired => write!(f, "demo_setup_required"),
            Self::AccountLocked => write!(f, "account_locked"),
        }
    }
}

/// Wire format for API error responses.
///
/// Every non-2xx response from the API returns this shape.
/// `details` carries per-field validation messages when present.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<HashMap<String, Vec<String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustive list of all ErrorCode variants.
    /// Update the array size when adding variants — the compiler enforces the count.
    fn all_error_codes() -> [ErrorCode; 27] {
        [
            ErrorCode::NotFound,
            ErrorCode::Conflict,
            ErrorCode::ValidationError,
            ErrorCode::InternalError,
            ErrorCode::ParseError,
            ErrorCode::NetworkError,
            ErrorCode::InvalidCredentials,
            ErrorCode::Unauthorized,
            ErrorCode::Forbidden,
            ErrorCode::InvalidToken,
            ErrorCode::SetupFailed,
            ErrorCode::RateLimited,
            ErrorCode::MethodNotAllowed,
            ErrorCode::ProductionDbExists,
            ErrorCode::NotMokumoDatabase,
            ErrorCode::DatabaseCorrupt,
            ErrorCode::SchemaIncompatible,
            ErrorCode::RestoreInProgress,
            ErrorCode::ShopLogoRequiresProductionProfile,
            ErrorCode::LogoFormatUnsupported,
            ErrorCode::LogoTooLarge,
            ErrorCode::LogoDimensionsExceeded,
            ErrorCode::LogoMalformed,
            ErrorCode::MissingField,
            ErrorCode::ShopLogoNotFound,
            ErrorCode::DemoSetupRequired,
            ErrorCode::AccountLocked,
        ]
    }

    #[test]
    fn export_bindings() {
        ErrorCode::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ErrorCode TypeScript bindings");
        ErrorBody::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ErrorBody TypeScript bindings");
    }

    #[test]
    fn error_code_serializes_to_snake_case() {
        let cases = [
            (ErrorCode::NotFound, "\"not_found\""),
            (ErrorCode::Conflict, "\"conflict\""),
            (ErrorCode::ValidationError, "\"validation_error\""),
            (ErrorCode::InternalError, "\"internal_error\""),
            (ErrorCode::ParseError, "\"parse_error\""),
            (ErrorCode::NetworkError, "\"network_error\""),
            (ErrorCode::InvalidCredentials, "\"invalid_credentials\""),
            (ErrorCode::Unauthorized, "\"unauthorized\""),
            (ErrorCode::Forbidden, "\"forbidden\""),
            (ErrorCode::InvalidToken, "\"invalid_token\""),
            (ErrorCode::SetupFailed, "\"setup_failed\""),
            (ErrorCode::RateLimited, "\"rate_limited\""),
            (ErrorCode::MethodNotAllowed, "\"method_not_allowed\""),
            (ErrorCode::ProductionDbExists, "\"production_db_exists\""),
            (ErrorCode::NotMokumoDatabase, "\"not_mokumo_database\""),
            (ErrorCode::DatabaseCorrupt, "\"database_corrupt\""),
            (ErrorCode::SchemaIncompatible, "\"schema_incompatible\""),
            (ErrorCode::RestoreInProgress, "\"restore_in_progress\""),
            (
                ErrorCode::ShopLogoRequiresProductionProfile,
                "\"shop_logo_requires_production_profile\"",
            ),
            (
                ErrorCode::LogoFormatUnsupported,
                "\"logo_format_unsupported\"",
            ),
            (ErrorCode::LogoTooLarge, "\"logo_too_large\""),
            (
                ErrorCode::LogoDimensionsExceeded,
                "\"logo_dimensions_exceeded\"",
            ),
            (ErrorCode::LogoMalformed, "\"logo_malformed\""),
            (ErrorCode::MissingField, "\"missing_field\""),
            (ErrorCode::ShopLogoNotFound, "\"shop_logo_not_found\""),
            (ErrorCode::DemoSetupRequired, "\"demo_setup_required\""),
            (ErrorCode::AccountLocked, "\"account_locked\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                expected,
                "Failed to serialize {variant:?}"
            );
        }
    }

    #[test]
    fn error_code_deserializes_from_snake_case() {
        let cases = [
            ("\"not_found\"", ErrorCode::NotFound),
            ("\"conflict\"", ErrorCode::Conflict),
            ("\"validation_error\"", ErrorCode::ValidationError),
            ("\"internal_error\"", ErrorCode::InternalError),
            ("\"parse_error\"", ErrorCode::ParseError),
            ("\"network_error\"", ErrorCode::NetworkError),
            ("\"invalid_credentials\"", ErrorCode::InvalidCredentials),
            ("\"unauthorized\"", ErrorCode::Unauthorized),
            ("\"forbidden\"", ErrorCode::Forbidden),
            ("\"invalid_token\"", ErrorCode::InvalidToken),
            ("\"setup_failed\"", ErrorCode::SetupFailed),
            ("\"rate_limited\"", ErrorCode::RateLimited),
            ("\"method_not_allowed\"", ErrorCode::MethodNotAllowed),
            ("\"production_db_exists\"", ErrorCode::ProductionDbExists),
            ("\"not_mokumo_database\"", ErrorCode::NotMokumoDatabase),
            ("\"database_corrupt\"", ErrorCode::DatabaseCorrupt),
            ("\"schema_incompatible\"", ErrorCode::SchemaIncompatible),
            ("\"restore_in_progress\"", ErrorCode::RestoreInProgress),
            (
                "\"shop_logo_requires_production_profile\"",
                ErrorCode::ShopLogoRequiresProductionProfile,
            ),
            (
                "\"logo_format_unsupported\"",
                ErrorCode::LogoFormatUnsupported,
            ),
            ("\"logo_too_large\"", ErrorCode::LogoTooLarge),
            (
                "\"logo_dimensions_exceeded\"",
                ErrorCode::LogoDimensionsExceeded,
            ),
            ("\"logo_malformed\"", ErrorCode::LogoMalformed),
            ("\"missing_field\"", ErrorCode::MissingField),
            ("\"shop_logo_not_found\"", ErrorCode::ShopLogoNotFound),
            ("\"demo_setup_required\"", ErrorCode::DemoSetupRequired),
            ("\"account_locked\"", ErrorCode::AccountLocked),
        ];
        for (json, expected) in cases {
            let code: ErrorCode = serde_json::from_str(json).unwrap();
            assert_eq!(code, expected, "Failed to deserialize {json}");
        }
    }

    #[test]
    fn unknown_error_code_rejected() {
        let result = serde_json::from_str::<ErrorCode>("\"unknown_code\"");
        assert!(result.is_err(), "Unknown error codes must be rejected");
    }

    #[test]
    fn error_code_display() {
        // Uses all_error_codes() so new variants are automatically covered
        for variant in all_error_codes() {
            let display = variant.to_string();
            assert!(
                !display.is_empty(),
                "Display for {variant:?} should not be empty"
            );
        }
        // Spot-check a few values
        assert_eq!(ErrorCode::NotFound.to_string(), "not_found");
        assert_eq!(
            ErrorCode::InvalidCredentials.to_string(),
            "invalid_credentials"
        );
        assert_eq!(ErrorCode::SetupFailed.to_string(), "setup_failed");
    }

    #[test]
    fn display_matches_serde_for_all_variants() {
        // Guard: if Display and serde diverge, this catches it.
        let all_variants = all_error_codes();
        for variant in all_variants {
            let serde_str = serde_json::to_string(&variant)
                .unwrap()
                .trim_matches('"')
                .to_string();
            assert_eq!(
                variant.to_string(),
                serde_str,
                "Display and serde disagree for {variant:?}"
            );
        }
    }

    #[test]
    fn serde_roundtrip_without_details() {
        let body = ErrorBody {
            code: ErrorCode::NotFound,
            message: "Customer not found".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        let restored: ErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.code, ErrorCode::NotFound);
        assert_eq!(restored.message, "Customer not found");
        assert!(restored.details.is_none());
    }

    #[test]
    fn serde_roundtrip_with_details() {
        let mut details = HashMap::new();
        details.insert(
            "email".into(),
            vec!["invalid format".into(), "required".into()],
        );
        details.insert("name".into(), vec!["too short".into()]);

        let body = ErrorBody {
            code: ErrorCode::ValidationError,
            message: "Validation failed".into(),
            details: Some(details),
        };
        let json = serde_json::to_string(&body).unwrap();
        let restored: ErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.code, ErrorCode::ValidationError);
        let d = restored.details.unwrap();
        assert_eq!(d["email"].len(), 2);
        assert_eq!(d["name"], vec!["too short"]);
    }

    #[test]
    fn details_serialized_as_null_when_none() {
        let body = ErrorBody {
            code: ErrorCode::NotFound,
            message: "Not found".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(
            json.contains("\"details\":null"),
            "details should serialize as null when None, got: {json}"
        );
    }

    #[test]
    fn wire_format_backward_compatible() {
        let body = ErrorBody {
            code: ErrorCode::NotFound,
            message: "Not found".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(
            json.contains("\"code\":\"not_found\""),
            "Wire format should use snake_case strings, got: {json}"
        );
    }

    mod proptest_roundtrips {
        use super::*;
        use proptest::prelude::*;

        fn arb_error_code() -> impl Strategy<Value = ErrorCode> {
            // Kept in sync with all_error_codes() — both use exhaustive lists.
            // Update when adding variants.
            prop_oneof![
                Just(ErrorCode::NotFound),
                Just(ErrorCode::Conflict),
                Just(ErrorCode::ValidationError),
                Just(ErrorCode::InternalError),
                Just(ErrorCode::ParseError),
                Just(ErrorCode::NetworkError),
                Just(ErrorCode::InvalidCredentials),
                Just(ErrorCode::Unauthorized),
                Just(ErrorCode::Forbidden),
                Just(ErrorCode::InvalidToken),
                Just(ErrorCode::SetupFailed),
                Just(ErrorCode::RateLimited),
                Just(ErrorCode::MethodNotAllowed),
                Just(ErrorCode::ProductionDbExists),
                Just(ErrorCode::NotMokumoDatabase),
                Just(ErrorCode::DatabaseCorrupt),
                Just(ErrorCode::SchemaIncompatible),
                Just(ErrorCode::RestoreInProgress),
                Just(ErrorCode::ShopLogoRequiresProductionProfile),
                Just(ErrorCode::LogoFormatUnsupported),
                Just(ErrorCode::LogoTooLarge),
                Just(ErrorCode::LogoDimensionsExceeded),
                Just(ErrorCode::LogoMalformed),
                Just(ErrorCode::MissingField),
                Just(ErrorCode::ShopLogoNotFound),
                Just(ErrorCode::DemoSetupRequired),
                Just(ErrorCode::AccountLocked),
            ]
        }

        proptest! {
            #[test]
            fn error_body_serialization_roundtrip(
                code in arb_error_code(),
                message in "[a-zA-Z ]{1,50}",
            ) {
                let original = ErrorBody {
                    code,
                    message,
                    details: None,
                };
                let json = serde_json::to_string(&original).unwrap();
                let restored: ErrorBody = serde_json::from_str(&json).unwrap();
                assert_eq!(original.code, restored.code);
                assert_eq!(original.message, restored.message);
            }
        }
    }
}
