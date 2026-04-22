#[path = "support/mod.rs"]
mod support;

use std::path::Path;
use support::StubGraft;

/// StubGraft compiles without implementing any lifecycle hooks — proves
/// the new methods have default no-op implementations (backward compatibility).
#[tokio::test]
async fn stub_graft_compiles_without_lifecycle_hooks() {
    let _graft = StubGraft::diamond();
    // If this test compiles, backward compatibility is proven.
}

/// Default lifecycle hooks return Ok(()) without side effects.
#[tokio::test]
async fn default_lifecycle_hooks_return_ok() {
    use kikan::Graft;

    let graft = StubGraft::diamond();

    let db_path = Path::new("/tmp/test.db");
    let backup_path = Path::new("/tmp/backup.db");
    let profile_dir = Path::new("/tmp/profiles/demo");

    assert!(graft.on_backup_created(db_path, backup_path).is_ok());
    assert!(graft.on_pre_restore(db_path, backup_path).is_ok());
    assert!(graft.on_post_restore(db_path, backup_path).is_ok());
    assert!(graft.on_post_reset_db(profile_dir).is_ok());
}

/// Default spawn_background_tasks is a no-op that returns immediately.
#[tokio::test]
async fn default_spawn_background_tasks_is_noop() {
    use kikan::Graft;
    use sea_orm::Database;

    let graft = StubGraft::diamond();
    let demo_db = Database::connect("sqlite::memory:").await.unwrap();
    let prod_db = Database::connect("sqlite::memory:").await.unwrap();
    let state = support::stub_app_state(demo_db, prod_db, "/tmp/test-bg".into());

    // Should complete immediately without panicking
    graft.spawn_background_tasks(&state);
}
