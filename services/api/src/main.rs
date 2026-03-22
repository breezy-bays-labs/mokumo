use axum::{Router, routing::get, Json, response::IntoResponse, http::StatusCode};
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Ensure data directory exists
    std::fs::create_dir_all(&cli.data_dir)?;

    let db_path = cli.data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = mokumo_db::initialize_database(&database_url).await?;
    tracing::info!("Database ready at {}", db_path.display());

    let state = Arc::new(AppState { db: pool });

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

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn serve_spa(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

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
