//! Per-request database handle, selected by session profile.
//!
//! `ProfileDb` is inserted into request extensions by the vertical's
//! profile-routing middleware (for mokumo: `profile_db_middleware` in
//! `services/api`). Handlers in protected routes extract the handle via
//! `ProfileDb(db): ProfileDb`, ensuring every request sees the database
//! chosen by its own session — not a snapshot captured at router-build
//! time. This is what preserves seamless profile switching: no restart,
//! no cross-profile bleed, and no handler code paths that can silently
//! bind to the wrong database.
//!
//! Kikan owns the **type** (so verticals can extract it without reaching
//! into another crate's private module) but not the **middleware** (the
//! middleware is welded to each vertical's `AppState` shape and so stays
//! in the vertical's service layer).

use axum::extract::FromRequestParts;
use axum::http::{StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};

use crate::db::DatabaseConnection;

/// Per-request database handle injected by the vertical's profile-routing
/// middleware.
///
/// Wraps `DatabaseConnection` directly — `sea_orm::DatabaseConnection` is
/// already Arc-backed internally, so no additional `Arc` wrapper is needed.
///
/// Handlers on protected routes extract this instead of going through a
/// router-level `State<_>` handle, ensuring each request always uses the
/// correct profile database regardless of the current global "active
/// profile" setting.
#[derive(Clone, Debug)]
pub struct ProfileDb(pub DatabaseConnection);

impl ProfileDb {
    /// Borrow the inner database connection.
    pub fn inner(&self) -> &DatabaseConnection {
        &self.0
    }
}

impl<S> FromRequestParts<S> for ProfileDb
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ProfileDb>()
            .cloned()
            .ok_or_else(missing_extension_response)
    }
}

/// Build a 500 response whose body matches the platform-wide `ErrorBody`
/// wire shape (`{"code":"internal_error","message":...,"details":null}`)
/// without pulling `kikan-types` into `kikan` (kikan-types already depends
/// on kikan for `SetupMode`; the dependency cannot reverse).
///
/// The message is intentionally generic — reaching this arm means the
/// vertical forgot to install its profile-routing middleware, which is a
/// programmer error, not a user-facing condition. The status code + code
/// string are the load-bearing parts.
fn missing_extension_response() -> Response {
    let body = serde_json::json!({
        "code": "internal_error",
        "message": "An internal error occurred",
        "details": null,
    });
    let mut response = (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(body)).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "no-store".parse().expect("static header value parses"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
