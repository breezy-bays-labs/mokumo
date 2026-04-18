//! Table-driven regression fixture for `ControlPlaneError` wire mapping.
//!
//! Every variant maps to a fixed `(ErrorCode, http_status)` tuple. Both the
//! HTTP adapter (`From<ControlPlaneError> for AppError`) and the UDS adapter
//! (`IntoResponse for ControlPlaneError`) must render the same tuple.
//!
//! Un-tagging a new variant without adding a fixture row — or drifting the
//! mapping between the two adapters — fails a test in this module. Pairs with
//! `tests/features/control_plane_error_variants.feature`.

use axum::body::to_bytes;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use kikan::{ConflictKind, ControlPlaneError};
use kikan_types::error::{ErrorBody, ErrorCode};

/// Build one exemplar of each `ControlPlaneError` variant. Exhaustive by
/// construction — pattern matching below fails to compile if the enum grows.
fn sample(variant: Variant) -> ControlPlaneError {
    match variant {
        Variant::NotFound => ControlPlaneError::NotFound,
        Variant::AlreadyBootstrapped => {
            ControlPlaneError::Conflict(ConflictKind::AlreadyBootstrapped)
        }
        Variant::LastAdminProtected => {
            ControlPlaneError::Conflict(ConflictKind::LastAdminProtected {
                message: "Cannot delete the last admin account. Assign another admin first.".into(),
            })
        }
        Variant::Validation => ControlPlaneError::Validation {
            field: "email".into(),
            message: "required".into(),
        },
        Variant::PermissionDenied => ControlPlaneError::PermissionDenied,
        Variant::Internal => ControlPlaneError::Internal(anyhow::anyhow!("db offline")),
    }
}

/// Enum-of-variants used by the exhaustiveness guard. One row per
/// `(ControlPlaneError, ConflictKind)` combination so each `ConflictKind`
/// has its own pinned wire tuple.
///
/// Adding a variant here WITHOUT covering every `ControlPlaneError` /
/// `ConflictKind` in `sample` fails the match. Adding a `ConflictKind`
/// without extending this enum fails the exhaustiveness test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    NotFound,
    AlreadyBootstrapped,
    LastAdminProtected,
    Validation,
    PermissionDenied,
    Internal,
}

const ALL_VARIANTS: &[Variant] = &[
    Variant::NotFound,
    Variant::AlreadyBootstrapped,
    Variant::LastAdminProtected,
    Variant::Validation,
    Variant::PermissionDenied,
    Variant::Internal,
];

fn expected_tuple(variant: Variant) -> (ErrorCode, u16) {
    match variant {
        Variant::NotFound => (ErrorCode::NotFound, 404),
        Variant::AlreadyBootstrapped => (ErrorCode::AlreadyBootstrapped, 409),
        Variant::LastAdminProtected => (ErrorCode::Conflict, 409),
        Variant::Validation => (ErrorCode::ValidationError, 400),
        Variant::PermissionDenied => (ErrorCode::Forbidden, 403),
        Variant::Internal => (ErrorCode::InternalError, 500),
    }
}

/// Exhaustiveness guard: the sample function must handle every variant listed,
/// and every variant must have a pinned tuple.
#[test]
fn every_variant_has_a_pinned_tuple() {
    for v in ALL_VARIANTS {
        let _ = sample(*v);
        let _ = expected_tuple(*v);
    }
}

/// Compile-time exhaustiveness guard against drift: when a new
/// `ConflictKind` variant lands, this wildcard-free match fails to
/// compile. The local `Variant` mirror + `ALL_VARIANTS` above is for
/// runtime iteration; this function anchors compile-time coverage to
/// the production `ConflictKind` enum directly.
#[allow(dead_code)]
fn conflict_kind_exhaustive_compile_check(k: ConflictKind) {
    match k {
        ConflictKind::AlreadyBootstrapped => {}
        ConflictKind::LastAdminProtected { .. } => {}
    }
}

#[tokio::test]
async fn handler_level_accessors_match_fixture() {
    for v in ALL_VARIANTS {
        let err = sample(*v);
        let (expected_code, expected_status) = expected_tuple(*v);
        assert_eq!(
            err.error_code(),
            expected_code,
            "error_code() mismatch for {v:?}"
        );
        assert_eq!(
            err.http_status(),
            expected_status,
            "http_status() mismatch for {v:?}"
        );
    }
}

#[tokio::test]
async fn uds_adapter_renders_pinned_tuple() {
    for v in ALL_VARIANTS {
        let err = sample(*v);
        let (expected_code, expected_status) = expected_tuple(*v);
        let response = err.into_response();
        assert_eq!(
            response.status(),
            StatusCode::from_u16(expected_status).unwrap(),
            "UDS adapter status mismatch for {v:?}"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body.code, expected_code,
            "UDS adapter code mismatch for {v:?}"
        );
    }
}

#[tokio::test]
async fn http_adapter_renders_pinned_tuple() {
    for v in ALL_VARIANTS {
        let err = sample(*v);
        let (expected_code, expected_status) = expected_tuple(*v);
        let app_err: kikan::AppError = err.into();
        let response = app_err.into_response();
        assert_eq!(
            response.status(),
            StatusCode::from_u16(expected_status).unwrap(),
            "HTTP adapter status mismatch for {v:?}"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body.code, expected_code,
            "HTTP adapter code mismatch for {v:?}"
        );
    }
}

#[tokio::test]
async fn internal_variant_redacts_wire_message_in_both_adapters() {
    let secret = "secret connection string exposed";

    let err = ControlPlaneError::Internal(anyhow::anyhow!("{secret}"));
    let response = err.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: ErrorBody = serde_json::from_slice(&body).unwrap();
    assert!(
        !body.message.contains("secret"),
        "UDS adapter leaked internal message: {}",
        body.message
    );

    let err = ControlPlaneError::Internal(anyhow::anyhow!("{secret}"));
    let app_err: kikan::AppError = err.into();
    let response = app_err.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: ErrorBody = serde_json::from_slice(&body).unwrap();
    assert!(
        !body.message.contains("secret"),
        "HTTP adapter leaked internal message: {}",
        body.message
    );
}

#[tokio::test]
async fn validation_variant_surfaces_field_in_details() {
    let err = ControlPlaneError::Validation {
        field: "email".into(),
        message: "required".into(),
    };
    let response = err.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: ErrorBody = serde_json::from_slice(&body).unwrap();
    let details = body.details.expect("validation must carry details");
    assert_eq!(details.get("email"), Some(&vec!["required".to_string()]));
}
