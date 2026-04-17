//! HTTP handlers for the shop-logo vertical.
//!
//! Mirrors the customer vertical: per-request DB via `kikan::ProfileDb`,
//! singleton dependencies in `ShopLogoRouterDeps`, mount site in the shell
//! (`services/api/src/lib.rs`). The production-profile guard and upload
//! rate-limit are both expressed here because they are policy of the
//! shop vertical — the shell simply forwards the active-profile handle
//! and the rate-limiter in the deps struct.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum_login::AuthSession;
use kikan::SetupMode;
use kikan::rate_limit::RateLimiter;
use kikan_types::error::ErrorCode;
use mokumo_core::actor::Actor;
use mokumo_core::error::DomainError;
use tokio::fs;
use uuid::Uuid;

use crate::shop::adapter::SqliteShopLogoRepository;
use crate::shop::error::ShopLogoHandlerError;
use crate::shop::logo_validator::LogoValidator;
use crate::shop::service::ShopLogoService;

#[derive(Clone)]
pub struct ShopLogoRouterDeps {
    pub activity_writer: Arc<dyn kikan::ActivityWriter>,
    /// The production profile database. `GET /api/shop/logo` reads here
    /// regardless of the caller's active profile — the shop logo is a
    /// production-profile-owned resource, and the public GET must return
    /// the same bytes whether called from Demo or Production UX
    /// (`setup_status` emits a cross-profile-stable `logo_url`).
    pub production_db: kikan::db::DatabaseConnection,
    pub data_dir: PathBuf,
    pub logo_upload_limiter: Arc<RateLimiter>,
}

/// Unauthenticated logo-fetch router. Mount under `/api/shop`.
pub fn shop_logo_public_router() -> Router<ShopLogoRouterDeps> {
    Router::new().route("/logo", get(get_logo))
}

/// Authenticated upload / delete router with a 3 MiB body limit.
///
/// The extra MiB above the 2 MiB `LogoValidator::MAX_BYTES` limit covers
/// multipart framing overhead — rejection past that point is the shell's
/// body-limit response, not the validator's.
pub fn shop_logo_protected_router() -> Router<ShopLogoRouterDeps> {
    Router::new()
        .route("/logo", post(post_logo).delete(delete_logo))
        .layer(axum::extract::DefaultBodyLimit::max(3 * 1024 * 1024))
}

fn build_service(
    db: kikan::db::DatabaseConnection,
    deps: &ShopLogoRouterDeps,
) -> ShopLogoService<SqliteShopLogoRepository> {
    ShopLogoService::new(SqliteShopLogoRepository::new(
        db,
        deps.activity_writer.clone(),
    ))
}

fn require_production(mode: SetupMode) -> Result<(), ShopLogoHandlerError> {
    if mode != SetupMode::Production {
        return Err(ShopLogoHandlerError::Forbidden {
            code: ErrorCode::ShopLogoRequiresProductionProfile,
            message: "Logo management requires the production profile".into(),
        });
    }
    Ok(())
}

fn require_auth(
    auth_session: &AuthSession<kikan::auth::Backend>,
) -> Result<i64, ShopLogoHandlerError> {
    auth_session
        .user
        .as_ref()
        .map(|u| u.user.id.get())
        .ok_or_else(|| ShopLogoHandlerError::Unauthorized {
            code: ErrorCode::Unauthorized,
            message: "Not authenticated".into(),
        })
}

/// Core logic for `GET /api/shop/logo`. Split out so it can be unit-tested
/// without spinning up an Axum router — the handler is a thin wrapper around
/// this free function.
pub(crate) async fn get_logo_impl(
    deps: &ShopLogoRouterDeps,
) -> Result<axum::response::Response, ShopLogoHandlerError> {
    let svc = build_service(deps.production_db.clone(), deps);
    let info = svc.get_logo_info().await?.ok_or_else(|| {
        ShopLogoHandlerError::from(DomainError::NotFound {
            entity: "shop_logo",
            id: "1".into(),
        })
    })?;

    let content_type = match info.extension.as_str() {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        other => {
            tracing::error!("get_logo: unknown extension stored: {other}");
            return Err(ShopLogoHandlerError::Unprocessable {
                code: ErrorCode::LogoMalformed,
                message: "Stored logo has an unknown format".into(),
            });
        }
    };

    let path = deps
        .data_dir
        .join("production")
        .join(format!("logo.{}", info.extension));

    let data = fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ShopLogoHandlerError::from(DomainError::NotFound {
                entity: "shop_logo",
                id: "1".into(),
            })
        } else {
            tracing::error!("get_logo: failed to read logo file {path:?}: {e}");
            ShopLogoHandlerError::Internal("Failed to read logo file".into())
        }
    })?;

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static(content_type),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        axum::http::header::ETAG,
        HeaderValue::from_str(&format!("\"{}\"", info.updated_at))
            .unwrap_or_else(|_| HeaderValue::from_static("\"\"")),
    );

    Ok((StatusCode::OK, headers, Bytes::from(data)).into_response())
}

/// Core logic for `POST /api/shop/logo`. Takes the already-validated multipart
/// bytes so it can be unit-tested without Axum's extractor machinery.
pub(crate) async fn upload_logo_impl(
    db: kikan::db::DatabaseConnection,
    deps: &ShopLogoRouterDeps,
    mode: SetupMode,
    actor_id: i64,
    bytes: Vec<u8>,
) -> Result<StatusCode, ShopLogoHandlerError> {
    require_production(mode)?;

    let actor_key = actor_id.to_string();
    if !deps.logo_upload_limiter.check_and_record(&actor_key) {
        return Err(ShopLogoHandlerError::TooManyRequests(
            "Too many logo upload attempts. Try again later.".into(),
        ));
    }

    let validated = LogoValidator::validate(bytes)?;
    let new_ext = validated.format.to_string();
    let production_dir = deps.data_dir.join("production");

    let svc = build_service(db, deps);
    let old_ext = svc.get_logo_info().await?.map(|info| info.extension);

    // Per-upload UUID prevents concurrent uploaders from racing on the same
    // tmp path and clobbering each other's bytes before rename.
    let tmp_path = production_dir.join(format!("logo.{new_ext}.{}.tmp", Uuid::new_v4()));
    let final_path = production_dir.join(format!("logo.{new_ext}"));

    fs::create_dir_all(&production_dir).await.map_err(|e| {
        tracing::error!("failed to create production dir: {e}");
        ShopLogoHandlerError::Internal("Failed to create logo directory".into())
    })?;

    fs::write(&tmp_path, &validated.bytes).await.map_err(|e| {
        tracing::error!("failed to write logo tmp: {e}");
        ShopLogoHandlerError::Internal("Failed to write logo file".into())
    })?;

    fs::rename(&tmp_path, &final_path).await.map_err(|e| {
        tracing::error!("failed to rename logo: {e}");
        ShopLogoHandlerError::Internal("Failed to persist logo file".into())
    })?;

    let updated_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let actor = Actor::user(actor_id);
    if let Err(e) = svc.upsert_logo(&new_ext, updated_at, &actor).await {
        // DB write failed after the rename succeeded — the final file would
        // otherwise be orphaned (no DB row pointing at it, or a stale row
        // pointing at a different extension). Best-effort cleanup.
        if let Err(rm) = fs::remove_file(&final_path).await
            && rm.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                "upload_logo: upsert failed and cleanup of {final_path:?} also failed: {rm}"
            );
        }
        return Err(e.into());
    }

    if let Some(ref old) = old_ext
        && old != &new_ext
    {
        let old_path = production_dir.join(format!("logo.{old}"));
        if let Err(e) = fs::remove_file(&old_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("failed to remove old logo file {old_path:?}: {e}");
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Core logic for `DELETE /api/shop/logo`.
pub(crate) async fn delete_logo_impl(
    db: kikan::db::DatabaseConnection,
    deps: &ShopLogoRouterDeps,
    mode: SetupMode,
    actor_id: i64,
) -> Result<StatusCode, ShopLogoHandlerError> {
    require_production(mode)?;

    let svc = build_service(db, deps);
    let info = svc.get_logo_info().await?.ok_or_else(|| {
        ShopLogoHandlerError::from(DomainError::NotFound {
            entity: "shop_logo",
            id: "1".into(),
        })
    })?;

    let actor = Actor::user(actor_id);
    svc.delete_logo(&actor).await?;

    let path = deps
        .data_dir
        .join("production")
        .join(format!("logo.{}", info.extension));

    if let Err(e) = fs::remove_file(&path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("delete_logo: failed to remove logo file {path:?}: {e}");
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn get_logo(
    State(deps): State<ShopLogoRouterDeps>,
) -> Result<axum::response::Response, ShopLogoHandlerError> {
    get_logo_impl(&deps).await
}

async fn post_logo(
    auth_session: AuthSession<kikan::auth::Backend>,
    kikan::ProfileDb(db): kikan::ProfileDb,
    kikan::ActiveProfile(mode): kikan::ActiveProfile,
    State(deps): State<ShopLogoRouterDeps>,
    mut multipart: axum::extract::Multipart,
) -> Result<StatusCode, ShopLogoHandlerError> {
    let actor_id = require_auth(&auth_session)?;
    let bytes = read_logo_field(&mut multipart).await?;
    upload_logo_impl(db, &deps, mode, actor_id, bytes.to_vec()).await
}

async fn delete_logo(
    auth_session: AuthSession<kikan::auth::Backend>,
    kikan::ProfileDb(db): kikan::ProfileDb,
    kikan::ActiveProfile(mode): kikan::ActiveProfile,
    State(deps): State<ShopLogoRouterDeps>,
) -> Result<StatusCode, ShopLogoHandlerError> {
    let actor_id = require_auth(&auth_session)?;
    delete_logo_impl(db, &deps, mode, actor_id).await
}

async fn read_logo_field(
    multipart: &mut axum::extract::Multipart,
) -> Result<Bytes, ShopLogoHandlerError> {
    loop {
        match multipart.next_field().await.map_err(|e| {
            tracing::warn!("multipart error: {e}");
            ShopLogoHandlerError::BadRequest {
                code: ErrorCode::MissingField,
                message: "Failed to read multipart data".into(),
            }
        })? {
            None => {
                return Err(ShopLogoHandlerError::BadRequest {
                    code: ErrorCode::MissingField,
                    message: "Required field 'logo' is missing".into(),
                });
            }
            Some(field) if field.name() == Some("logo") => {
                return field.bytes().await.map_err(|e| {
                    tracing::warn!("failed to read logo field: {e}");
                    ShopLogoHandlerError::BadRequest {
                        code: ErrorCode::MissingField,
                        message: "Failed to read logo data".into(),
                    }
                });
            }
            Some(_) => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kikan::SqliteActivityWriter;
    use std::time::Duration;

    fn minimal_png(width: u32, height: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        let ihdr = {
            let mut d = Vec::new();
            d.extend_from_slice(&width.to_be_bytes());
            d.extend_from_slice(&height.to_be_bytes());
            d.extend_from_slice(&[8, 2, 0, 0, 0]);
            d
        };
        write_chunk(&mut buf, b"IHDR", &ihdr);
        write_chunk(&mut buf, b"IDAT", &[0x78, 0x9c, 0]);
        write_chunk(&mut buf, b"IEND", &[]);
        buf
    }

    fn minimal_jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut buf = vec![0xFF, 0xD8, 0xFF, 0xC0];
        let sof_len: u16 = 11;
        buf.extend_from_slice(&sof_len.to_be_bytes());
        buf.push(8);
        buf.extend_from_slice(&height.to_be_bytes());
        buf.extend_from_slice(&width.to_be_bytes());
        buf.extend_from_slice(&[1, 1, 0x11, 0]);
        buf.extend_from_slice(&[0xFF, 0xD9]);
        buf
    }

    fn write_chunk(buf: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
        buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
        buf.extend_from_slice(tag);
        buf.extend_from_slice(data);
        buf.extend_from_slice(&[0u8; 4]);
    }

    async fn test_db() -> kikan::db::DatabaseConnection {
        let tmp_url = format!(
            "sqlite:{}?mode=rwc",
            tempfile::NamedTempFile::new().unwrap().path().display()
        );
        crate::db::initialize_database(&tmp_url).await.unwrap()
    }

    fn make_deps(db: kikan::db::DatabaseConnection, data_dir: PathBuf) -> ShopLogoRouterDeps {
        ShopLogoRouterDeps {
            activity_writer: Arc::new(SqliteActivityWriter::new()),
            production_db: db,
            data_dir,
            logo_upload_limiter: Arc::new(RateLimiter::new(10, Duration::from_secs(60))),
        }
    }

    #[tokio::test]
    async fn get_logo_returns_404_when_no_logo_stored() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db, tmp.path().to_path_buf());

        let err = get_logo_impl(&deps).await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn upload_then_get_logo_returns_png_bytes() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        let png = minimal_png(32, 32);
        let status = upload_logo_impl(db.clone(), &deps, SetupMode::Production, 1, png.clone())
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NO_CONTENT);

        let response = get_logo_impl(&deps).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "image/png"
        );
        assert!(response.headers().get(axum::http::header::ETAG).is_some());

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), png.as_slice());
    }

    #[tokio::test]
    async fn upload_logo_rejected_in_demo_profile() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        let err = upload_logo_impl(db, &deps, SetupMode::Demo, 1, minimal_png(16, 16))
            .await
            .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn upload_logo_enforces_rate_limit() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let mut deps = make_deps(db.clone(), tmp.path().to_path_buf());
        deps.logo_upload_limiter = Arc::new(RateLimiter::new(1, Duration::from_secs(60)));

        upload_logo_impl(
            db.clone(),
            &deps,
            SetupMode::Production,
            7,
            minimal_png(16, 16),
        )
        .await
        .unwrap();

        let err = upload_logo_impl(db, &deps, SetupMode::Production, 7, minimal_png(16, 16))
            .await
            .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn upload_logo_rejects_invalid_bytes() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        let err = upload_logo_impl(
            db,
            &deps,
            SetupMode::Production,
            1,
            b"not an image".to_vec(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.into_response().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn upload_logo_replaces_previous_extension_file() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        upload_logo_impl(
            db.clone(),
            &deps,
            SetupMode::Production,
            1,
            minimal_png(16, 16),
        )
        .await
        .unwrap();
        assert!(tmp.path().join("production/logo.png").exists());

        upload_logo_impl(
            db.clone(),
            &deps,
            SetupMode::Production,
            2,
            minimal_jpeg(16, 16),
        )
        .await
        .unwrap();

        assert!(tmp.path().join("production/logo.jpeg").exists());
        assert!(
            !tmp.path().join("production/logo.png").exists(),
            "old-extension file should be cleaned up after extension change"
        );
    }

    #[tokio::test]
    async fn delete_logo_returns_404_when_none_stored() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        let err = delete_logo_impl(db, &deps, SetupMode::Production, 1)
            .await
            .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_logo_rejected_in_demo_profile() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        let err = delete_logo_impl(db, &deps, SetupMode::Demo, 1)
            .await
            .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn delete_logo_removes_file_and_db_row() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        upload_logo_impl(
            db.clone(),
            &deps,
            SetupMode::Production,
            1,
            minimal_png(16, 16),
        )
        .await
        .unwrap();
        assert!(tmp.path().join("production/logo.png").exists());

        let status = delete_logo_impl(db.clone(), &deps, SetupMode::Production, 1)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!tmp.path().join("production/logo.png").exists());

        // Second delete 404s — logo is gone from the DB too.
        let err = delete_logo_impl(db, &deps, SetupMode::Production, 1)
            .await
            .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_logo_reports_404_when_db_row_present_but_file_missing() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let deps = make_deps(db.clone(), tmp.path().to_path_buf());

        upload_logo_impl(
            db.clone(),
            &deps,
            SetupMode::Production,
            1,
            minimal_png(16, 16),
        )
        .await
        .unwrap();

        // Simulate disk drift: file removed out-of-band.
        tokio::fs::remove_file(tmp.path().join("production/logo.png"))
            .await
            .unwrap();

        let err = get_logo_impl(&deps).await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
    }
}
