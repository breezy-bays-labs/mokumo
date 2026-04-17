//! Shop-vertical error codes — the I1-compliant home for error variants that
//! name shop-domain concepts (shop logo, multipart field validation tied to
//! shop uploads, etc.).
//!
//! Wire-compatible with `kikan_types::error::ErrorBody`: both `ErrorCode` and
//! `ShopErrorCode` serialize to snake_case strings inside the same
//! `{"code": "...", "message": "...", "details": null}` envelope. Frontend
//! consumers widen their union to `ErrorCode | ShopErrorCode`.
//!
//! Moved from `kikan-types::error::ErrorCode` during Stage 3 S4.3 (#507) to
//! satisfy the I1 domain-purity invariant on kikan/kikan-types. Error-code
//! wire strings are preserved byte-for-byte — Hurl smoke tests assert on
//! `$.code`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Shop-vertical machine-readable error code for API responses.
///
/// Serializes to snake_case strings (e.g. `LogoTooLarge` → `"logo_too_large"`),
/// preserving the historical wire format byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ShopErrorCode {
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
}

impl std::fmt::Display for ShopErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShopLogoRequiresProductionProfile => {
                write!(f, "shop_logo_requires_production_profile")
            }
            Self::LogoFormatUnsupported => write!(f, "logo_format_unsupported"),
            Self::LogoTooLarge => write!(f, "logo_too_large"),
            Self::LogoDimensionsExceeded => write!(f, "logo_dimensions_exceeded"),
            Self::LogoMalformed => write!(f, "logo_malformed"),
            Self::MissingField => write!(f, "missing_field"),
            Self::ShopLogoNotFound => write!(f, "shop_logo_not_found"),
        }
    }
}

/// Shop-vertical wire shape mirroring `kikan_types::error::ErrorBody` but
/// keyed by `ShopErrorCode`. Serializes to byte-identical JSON when the code
/// strings match, so Hurl smoke tests see no difference.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShopErrorBody {
    pub code: ShopErrorCode,
    pub message: String,
    pub details: Option<HashMap<String, Vec<String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_shop_error_codes() -> [ShopErrorCode; 7] {
        [
            ShopErrorCode::ShopLogoRequiresProductionProfile,
            ShopErrorCode::LogoFormatUnsupported,
            ShopErrorCode::LogoTooLarge,
            ShopErrorCode::LogoDimensionsExceeded,
            ShopErrorCode::LogoMalformed,
            ShopErrorCode::MissingField,
            ShopErrorCode::ShopLogoNotFound,
        ]
    }

    #[test]
    fn export_bindings() {
        ShopErrorCode::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ShopErrorCode TypeScript bindings");
        ShopErrorBody::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export ShopErrorBody TypeScript bindings");
    }

    #[test]
    fn shop_error_code_serializes_to_snake_case() {
        let cases = [
            (
                ShopErrorCode::ShopLogoRequiresProductionProfile,
                "\"shop_logo_requires_production_profile\"",
            ),
            (
                ShopErrorCode::LogoFormatUnsupported,
                "\"logo_format_unsupported\"",
            ),
            (ShopErrorCode::LogoTooLarge, "\"logo_too_large\""),
            (
                ShopErrorCode::LogoDimensionsExceeded,
                "\"logo_dimensions_exceeded\"",
            ),
            (ShopErrorCode::LogoMalformed, "\"logo_malformed\""),
            (ShopErrorCode::MissingField, "\"missing_field\""),
            (ShopErrorCode::ShopLogoNotFound, "\"shop_logo_not_found\""),
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
    fn shop_error_code_deserializes_from_snake_case() {
        let cases = [
            (
                "\"shop_logo_requires_production_profile\"",
                ShopErrorCode::ShopLogoRequiresProductionProfile,
            ),
            (
                "\"logo_format_unsupported\"",
                ShopErrorCode::LogoFormatUnsupported,
            ),
            ("\"logo_too_large\"", ShopErrorCode::LogoTooLarge),
            (
                "\"logo_dimensions_exceeded\"",
                ShopErrorCode::LogoDimensionsExceeded,
            ),
            ("\"logo_malformed\"", ShopErrorCode::LogoMalformed),
            ("\"missing_field\"", ShopErrorCode::MissingField),
            ("\"shop_logo_not_found\"", ShopErrorCode::ShopLogoNotFound),
        ];
        for (json, expected) in cases {
            let code: ShopErrorCode = serde_json::from_str(json).unwrap();
            assert_eq!(code, expected, "Failed to deserialize {json}");
        }
    }

    #[test]
    fn display_matches_serde_for_all_variants() {
        for variant in all_shop_error_codes() {
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
    fn details_serialized_as_null_when_none() {
        let body = ShopErrorBody {
            code: ShopErrorCode::ShopLogoNotFound,
            message: "No logo".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(
            json.contains("\"details\":null"),
            "details should serialize as null when None, got: {json}"
        );
    }

    #[test]
    fn wire_format_matches_platform_envelope() {
        // Same {"code": "...", "message": "...", "details": null} shape used
        // by `kikan_types::error::ErrorBody`.
        let body = ShopErrorBody {
            code: ShopErrorCode::LogoTooLarge,
            message: "too big".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"code\":\"logo_too_large\""));
        assert!(json.contains("\"message\":\"too big\""));
        assert!(json.contains("\"details\":null"));
    }
}
