//! Shared test helpers for top-level integration tests.
//!
//! Each integration test in `tests/` compiles as a separate binary. This
//! module is included via `mod common;` in each test that needs a server
//! built through `Engine::boot` (the only public path after PR 4b).

use std::path::PathBuf;

use axum::Router;
use sea_orm::DatabaseConnection;
use tokio_util::sync::CancellationToken;

/// Boot a full app router using `Engine::boot` with pre-seeded databases.
///
/// Returns `(router, setup_token)`. The caller is responsible for keeping
/// the `CancellationToken` and any temporary directories alive while the
/// router is in use.
pub async fn boot_router(
    data_dir: PathBuf,
    recovery_dir: PathBuf,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: kikan::SetupMode,
    shutdown_token: CancellationToken,
) -> (Router, Option<String>) {
    let session_db_path = data_dir.join("sessions.db");
    let session_url = format!("sqlite:{}?mode=rwc", session_db_path.display());
    let _session_pool = kikan::db::open_raw_sqlite_pool(&session_url)
        .await
        .expect("failed to open session database");

    let (session_store, setup_completed, setup_token) =
        mokumo_shop::startup::init_session_and_setup(&production_db, &session_db_path)
            .await
            .expect("failed to init session store + setup token");

    let demo_install_ok =
        mokumo_shop::startup::resolve_demo_install_ok(&demo_db, active_profile).await;

    let graft = mokumo_shop::graft::MokumoApp;
    let profile_initializer: kikan::platform_state::SharedProfileDbInitializer =
        std::sync::Arc::new(mokumo_shop::profile_db_init::MokumoProfileDbInitializer);

    let boot_config = kikan::BootConfig::new(data_dir);

    let (engine, app_state) = kikan::Engine::<mokumo_shop::graft::MokumoApp>::boot(
        boot_config,
        &graft,
        demo_db,
        production_db,
        active_profile,
        session_store,
        profile_initializer,
        setup_completed,
        setup_token.clone(),
        demo_install_ok,
        recovery_dir,
        shutdown_token,
    )
    .await
    .expect("Engine::boot failed");

    {
        use kikan::Graft;
        graft.spawn_background_tasks(&app_state);
    }

    let router = engine.build_router(app_state);
    (router, setup_token)
}
