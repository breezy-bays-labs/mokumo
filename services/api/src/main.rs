use axum::{Router, routing::get, Json, extract::State, response::IntoResponse, http::StatusCode};
use clap::Parser;
use rust_embed::Embed;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use mokumo_types::HealthResponse;

#[derive(Parser)]
#[command(name = "mokumo", about = "Mokumo Print — production management server")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Address to bind to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Directory for application data (database, uploads)
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
}

#[derive(Embed)]
#[folder = "../../apps/web/build"]
struct SpaAssets;

struct AppState {
    db: sqlx::SqlitePool,
}

type SharedState = Arc<AppState>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let filter = match EnvFilter::try_from_default_env() {
        Ok(f) => f,
        Err(e) => {
            if std::env::var("RUST_LOG").is_ok() {
                eprintln!("WARNING: Invalid RUST_LOG value, falling back to 'info': {e}");
            }
            "info".into()
        }
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    std::fs::create_dir_all(&cli.data_dir)?;

    let db_path = cli.data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = mokumo_db::initialize_database(&database_url).await?;
    tracing::info!("Database ready at {}", db_path.display());

    let state: SharedState = Arc::new(AppState { db: pool });

    let app = Router::new()
        .route("/api/health", get(health))
        .fallback(serve_spa)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {addr}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health(
    State(state): State<SharedState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("Health check DB query failed: {e}");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    Ok(Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }))
}

async fn serve_spa(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Return a proper JSON 404 for unmatched API paths instead of serving the SPA shell
    if path.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            [
                (axum::http::header::CONTENT_TYPE, "application/json".to_owned()),
                (axum::http::header::CACHE_CONTROL, "no-store".to_owned()),
            ],
            r#"{"error":"not_found","message":"No API route matches this path"}"#
                .as_bytes()
                .to_vec(),
        );
    }

    if let Some(file) = SpaAssets::get(path) {
        let cache = if path.contains("/_app/immutable/") {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=3600"
        };
        (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
                (axum::http::header::CACHE_CONTROL, cache.to_owned()),
            ],
            file.data.to_vec(),
        )
    } else if let Some(index) = SpaAssets::get("index.html") {
        (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, index.metadata.mimetype().to_owned()),
                (axum::http::header::CACHE_CONTROL, "no-cache".to_owned()),
            ],
            index.data.to_vec(),
        )
    } else {
        tracing::warn!("SPA assets not found — run: moon run web:build");
        (
            StatusCode::NOT_FOUND,
            [
                (axum::http::header::CONTENT_TYPE, "text/plain".to_owned()),
                (axum::http::header::CACHE_CONTROL, "no-store".to_owned()),
            ],
            b"SPA not built. Run: moon run web:build".to_vec(),
        )
    }
}
