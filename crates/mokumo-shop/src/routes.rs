//! Data-plane route assembly for the Mokumo application.
//!
//! `data_plane_routes` returns the full Axum router for all domain routes,
//! including both public and protected endpoints. The engine wraps this
//! with the 5-layer middleware stack (host allowlist, security headers,
//! tracing, auth, profile-db).

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use kikan::AppError;
use kikan_types::HealthResponse;

use crate::state::SharedMokumoState;

type SharedState = SharedMokumoState;

/// Assemble the full data-plane router for the Mokumo application.
///
/// Includes public routes (health, server-info, auth, setup, restore,
/// backup-status, shop logo) and protected routes (customers, activity,
/// settings, ws, profile-switch, users, auth-me, demo-reset, diagnostics).
///
/// The caller (Engine::build_router) wraps this with middleware layers.
pub fn data_plane_routes(state: &SharedState) -> Router<SharedState> {
    let control_plane_state = state.control_plane_state();

    // ── Shop logo deps ──────────────────────────────────────────────
    let shop_logo_deps = crate::ShopLogoRouterDeps {
        activity_writer: state.activity_writer().clone(),
        production_db: state.production_db().clone(),
        data_dir: state.data_dir().clone(),
        logo_upload_limiter: Arc::new(kikan::rate_limit::RateLimiter::new(
            10,
            std::time::Duration::from_secs(60),
        )),
    };
    let shop_upload_router = Router::new().nest(
        "/api/shop",
        crate::shop_logo_protected_router().with_state(shop_logo_deps.clone()),
    );

    // ── Protected auth sub-router ───────────────────────────────────
    let protected_auth_routes = Router::new()
        .nest("/api/auth", kikan::platform::auth::auth_me_router())
        .route(
            "/api/account/recovery-codes/regenerate",
            post(kikan::platform::auth::regenerate_recovery_codes),
        )
        .with_state(control_plane_state.clone());

    // ── Protected routes (require login) ────────────────────────────
    let protected_routes = Router::new()
        .nest(
            "/api/customers",
            crate::customer_router().with_state(crate::CustomerRouterDeps {
                activity_writer: state.activity_writer().clone(),
            }),
        )
        .nest(
            "/api/users",
            kikan::platform::users::user_admin_router().with_state(control_plane_state.clone()),
        )
        .nest(
            "/api/activity",
            kikan::platform::activity_http::activity_router(),
        )
        .nest("/api/settings", crate::settings::router())
        .route(
            "/api/profile/switch",
            post(crate::profile_switch::profile_switch),
        )
        .route("/ws", get(crate::ws::ws_handler))
        .merge(protected_auth_routes)
        .merge(shop_upload_router)
        .merge(kikan::platform_protected_routes().with_state(state.platform_state()))
        .route_layer(axum::middleware::from_fn_with_state(
            state.platform_state(),
            kikan::platform::auth::require_auth_with_demo_auto_login,
        ));

    // ── Restore routes (unauthenticated, large body limit) ──────────
    let restore_routes = Router::new()
        .route(
            "/api/shop/restore/validate",
            post(crate::restore_handler::validate_handler),
        )
        .route(
            "/api/shop/restore",
            post(crate::restore_handler::restore_handler),
        )
        .layer(axum::extract::DefaultBodyLimit::max(500 * 1024 * 1024));

    // ── Public routes ───────────────────────────────────────────────
    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/server-info", get(crate::server_info::handler))
        .route("/api/setup-status", get(setup_status))
        .merge(kikan::platform_public_routes().with_state(state.platform_state()))
        .nest(
            "/api/shop",
            crate::shop_logo_public_router().with_state(shop_logo_deps),
        )
        .nest(
            "/api/auth",
            kikan::platform::auth::auth_router().with_state(control_plane_state.clone()),
        )
        .nest(
            "/api/setup",
            crate::setup::vertical_setup_router().with_state(control_plane_state.clone()),
        )
        .merge(restore_routes)
        .merge(protected_routes);

    #[cfg(debug_assertions)]
    {
        router = router
            .route("/api/debug/connections", get(crate::ws::debug_connections))
            .route("/api/debug/broadcast", post(crate::ws::debug_broadcast))
            .route("/api/debug/expire-pin", post(debug_expire_pin))
            .route("/api/debug/recovery-dir", get(debug_recovery_dir));
    }

    router.method_not_allowed_fallback(handle_method_not_allowed)
}

// ── Handler functions ───────────────────────────────────────────────────────

async fn health(
    State(state): State<SharedState>,
) -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<HealthResponse>,
    ),
    AppError,
> {
    use kikan_types::SetupMode;

    kikan::db::health_check(state.db_for(SetupMode::Demo)).await?;
    kikan::db::health_check(state.db_for(SetupMode::Production)).await?;

    let active = state.active_profile_mode();

    let install_ok = if active == SetupMode::Production {
        true
    } else {
        state
            .demo_install_ok()
            .load(std::sync::atomic::Ordering::Acquire)
    };

    let db_path = state
        .data_dir()
        .join(active.as_dir_name())
        .join("mokumo.db");
    let disk_warning = kikan::platform::diagnostics::compute_disk_warning(state.data_dir());
    let diag_result =
        tokio::task::spawn_blocking(move || kikan::db::diagnose_database(&db_path)).await;
    let storage_ok = match diag_result {
        Ok(Ok(diag)) => !disk_warning && !diag.vacuum_needed(),
        Ok(Err(e)) => {
            tracing::warn!("diagnose_database failed in health handler: {e}");
            false
        }
        Err(e) => {
            tracing::warn!("spawn_blocking panicked in health handler: {e}");
            false
        }
    };

    let uptime_seconds = state.started_at().elapsed().as_secs();
    let status = if install_ok && storage_ok {
        "ok"
    } else {
        "degraded"
    };

    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(HealthResponse {
            status: status.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            uptime_seconds,
            database: "ok".into(),
            install_ok,
            storage_ok,
        }),
    ))
}

async fn setup_status(
    State(state): State<SharedState>,
) -> Result<Json<kikan_types::setup::SetupStatusResponse>, AppError> {
    let active = state.active_profile_mode();
    let setup_complete = state.is_setup_complete();
    let is_first_launch = state
        .is_first_launch()
        .load(std::sync::atomic::Ordering::Acquire);

    let shop_name = crate::db::get_shop_name(state.production_db())
        .await
        .map_err(|e| {
            tracing::error!("setup_status: failed to fetch shop_name: {e}");
            AppError::InternalError("Failed to read shop configuration".into())
        })?;

    let production_setup_complete = crate::db::is_setup_complete(state.production_db())
        .await
        .map_err(|e| {
            tracing::error!("setup_status: failed to fetch production_setup_complete: {e}");
            AppError::InternalError("Failed to read production setup status".into())
        })?;

    let logo_info: Option<(Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT logo_extension, logo_epoch FROM shop_settings WHERE id = 1")
            .fetch_optional(state.production_db().get_sqlite_connection_pool())
            .await
            .map_err(|e| {
                tracing::error!("setup_status: failed to fetch logo_info: {e}");
                AppError::InternalError("Failed to read shop logo".into())
            })?;

    let logo_url = logo_info.and_then(|(ext, ts)| match (ext, ts) {
        (Some(_), Some(updated_at)) => Some(format!("/api/shop/logo?v={updated_at}")),
        _ => None,
    });

    Ok(Json(kikan_types::setup::SetupStatusResponse {
        setup_complete,
        setup_mode: setup_complete.then_some(active),
        is_first_launch,
        production_setup_complete,
        shop_name,
        logo_url,
    }))
}

async fn handle_method_not_allowed() -> Response {
    let body = kikan_types::error::ErrorBody {
        code: kikan_types::error::ErrorCode::MethodNotAllowed,
        message: "Method not allowed".into(),
        details: None,
    };
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response()
}

#[cfg(debug_assertions)]
async fn debug_recovery_dir(State(state): State<SharedState>) -> impl IntoResponse {
    Json(serde_json::json!({"path": state.recovery_dir().to_string_lossy()}))
}

#[cfg(debug_assertions)]
async fn debug_expire_pin(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let email = body["email"].as_str().unwrap_or_default();
    if let Some(mut entry) = state.reset_pins().get_mut(email) {
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(20 * 60);
        entry.created_at = past;
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}
