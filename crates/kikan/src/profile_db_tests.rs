use super::*;
use kikan_types::SetupMode;

#[tokio::test]
async fn from_request_parts_returns_err_when_extension_absent() {
    use axum::http::Request;

    let req = Request::builder().body(axum::body::Body::empty()).unwrap();
    let (mut parts, _) = req.into_parts();

    let result = ProfileDb::from_request_parts(&mut parts, &()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn rejection_body_matches_platform_error_wire_shape() {
    use axum::http::Request;

    let req = Request::builder().body(axum::body::Body::empty()).unwrap();
    let (mut parts, _) = req.into_parts();

    let response = ProfileDb::from_request_parts(&mut parts, &())
        .await
        .expect_err("missing extension must reject");

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "internal_error");
    assert_eq!(json["message"], "An internal error occurred");
    assert!(json["details"].is_null());
}

#[tokio::test]
async fn active_profile_extractor_rejects_when_missing() {
    use axum::http::Request;

    let req = Request::builder().body(axum::body::Body::empty()).unwrap();
    let (mut parts, _) = req.into_parts();

    let result = ActiveProfile::<SetupMode>::from_request_parts(&mut parts, &()).await;
    assert_eq!(
        result.unwrap_err().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn active_profile_extractor_returns_inserted_value() {
    use axum::http::Request;

    let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
    req.extensions_mut().insert(ActiveProfile(SetupMode::Demo));
    let (mut parts, _) = req.into_parts();

    let ActiveProfile(mode) = ActiveProfile::<SetupMode>::from_request_parts(&mut parts, &())
        .await
        .expect("extractor should succeed when extension present");
    assert_eq!(mode, SetupMode::Demo);

    let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ActiveProfile(SetupMode::Production));
    let (mut parts, _) = req.into_parts();

    let ActiveProfile(mode) = ActiveProfile::<SetupMode>::from_request_parts(&mut parts, &())
        .await
        .expect("extractor should succeed when extension present");
    assert_eq!(mode, SetupMode::Production);
}

/// Verify that from_request_parts returns the exact ProfileDb that was
/// inserted, and that two distinct databases inserted for demo vs
/// production sessions are correctly routed — the extracted handle
/// queries the intended database.
#[tokio::test]
async fn routing_returns_correct_db_per_profile() {
    use crate::db::{DatabaseConnection, initialize_database};
    use axum::http::Request;

    async fn user_version(db: &DatabaseConnection) -> i64 {
        let pool = db.get_sqlite_connection_pool();
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(pool)
            .await
            .expect("user_version query failed")
    }

    async fn set_user_version(db: &DatabaseConnection, v: i64) {
        let pool = db.get_sqlite_connection_pool();
        sqlx::query(&format!("PRAGMA user_version = {v}"))
            .execute(pool)
            .await
            .expect("set user_version failed");
    }

    let demo_db = initialize_database("sqlite::memory:?mode=rwc")
        .await
        .unwrap();
    let prod_db = initialize_database("sqlite::memory:?mode=rwc")
        .await
        .unwrap();

    set_user_version(&demo_db, 1).await;
    set_user_version(&prod_db, 2).await;

    // Demo session
    let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
    req.extensions_mut().insert(ProfileDb(demo_db));
    let (mut parts, _) = req.into_parts();
    let ProfileDb(extracted) = ProfileDb::from_request_parts(&mut parts, &())
        .await
        .unwrap();
    assert_eq!(
        user_version(&extracted).await,
        1,
        "demo session should use the demo DB"
    );

    // Production session
    let mut req = Request::builder().body(axum::body::Body::empty()).unwrap();
    req.extensions_mut().insert(ProfileDb(prod_db));
    let (mut parts, _) = req.into_parts();
    let ProfileDb(extracted) = ProfileDb::from_request_parts(&mut parts, &())
        .await
        .unwrap();
    assert_eq!(
        user_version(&extracted).await,
        2,
        "production session should use the production DB"
    );
}
