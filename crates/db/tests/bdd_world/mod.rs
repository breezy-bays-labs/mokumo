use cucumber::{World, given, then, when};
use mokumo_core::activity::ActivityEntry;
use mokumo_core::customer::Customer;
use mokumo_core::error::DomainError;
use mokumo_core::sequence::FormattedSequence;
use mokumo_core::sequence::traits::SequenceGenerator;
use mokumo_db::DatabaseConnection;
use mokumo_db::sequence::SqliteSequenceGenerator;
use sqlx::SqlitePool;
use std::collections::HashSet;

mod customer_steps;
mod install_validation_steps;
mod migration_safety_steps;
mod restore_steps;
mod shop_logo_steps;

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct DbWorld {
    db: DatabaseConnection,
    pool: SqlitePool,
    generator: SqliteSequenceGenerator,
    result: Option<Result<FormattedSequence, DomainError>>,
    results: Vec<Result<FormattedSequence, DomainError>>,
    last_seeded_name: Option<String>,
    _tmp: tempfile::TempDir,
    // Customer transaction atomicity test state
    last_customer: Option<Customer>,
    last_error: Option<DomainError>,
    // Shop logo test state
    last_logo_error: Option<DomainError>,
    activity_query_result: Option<(Vec<ActivityEntry>, i64)>,
    // Migration safety scenario state
    ms_tmp: Option<tempfile::TempDir>,
    ms_db_path: Option<std::path::PathBuf>,
    ms_backup_path: Option<std::path::PathBuf>,
    ms_oldest_backup: Option<std::path::PathBuf>,
    ms_source_seaql_count: Option<i64>,
    ms_backup_seaql_before_upgrade: Option<i64>,
    ms_migration_failed: bool,
    ms_table_count_before: Option<i64>,
    // Install validation test state
    pub last_validation_result: Option<bool>,
    // Restore step state
    pub restore_tmp: Option<tempfile::TempDir>,
    pub restore_candidate_path: Option<std::path::PathBuf>,
    pub restore_validate_result:
        Option<Result<mokumo_db::restore::CandidateInfo, mokumo_db::restore::RestoreError>>,
    pub restore_copy_result: Option<Result<(), mokumo_db::restore::RestoreError>>,
    pub restore_production_dir: Option<std::path::PathBuf>,
}

impl DbWorld {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = tmp.path().join("test.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = mokumo_db::initialize_database(&database_url)
            .await
            .expect("failed to initialize database");
        let pool = db.get_sqlite_connection_pool().clone();
        let generator = SqliteSequenceGenerator::new(pool.clone());
        Self {
            db,
            pool,
            generator,
            result: None,
            results: Vec::new(),
            last_seeded_name: None,
            _tmp: tmp,
            last_customer: None,
            last_error: None,
            last_logo_error: None,
            activity_query_result: None,
            ms_tmp: None,
            ms_db_path: None,
            ms_backup_path: None,
            ms_oldest_backup: None,
            ms_source_seaql_count: None,
            ms_backup_seaql_before_upgrade: None,
            ms_migration_failed: false,
            ms_table_count_before: None,
            last_validation_result: None,
            restore_tmp: None,
            restore_candidate_path: None,
            restore_validate_result: None,
            restore_copy_result: None,
            restore_production_dir: None,
        }
    }
}

// ---- Given steps ----

#[given(expr = "the customer sequence is seeded with prefix {string} and padding {int}")]
async fn customer_sequence_seeded(w: &mut DbWorld, prefix: String, padding: i32) {
    // Verify the migration-seeded values match what the scenario expects.
    let row = sqlx::query_as::<_, (String, i64)>(
        "SELECT prefix, padding FROM number_sequences WHERE name = 'customer'",
    )
    .fetch_one(&w.pool)
    .await
    .expect("customer sequence should be seeded by migration");
    assert_eq!(row.0, prefix, "Migration prefix doesn't match scenario");
    assert_eq!(
        row.1, padding as i64,
        "Migration padding doesn't match scenario"
    );
}

#[given(expr = "a sequence with prefix {string} and padding {int}")]
async fn sequence_with_prefix(w: &mut DbWorld, prefix: String, padding: i32) {
    // Insert a custom sequence for this scenario
    let name = prefix.to_lowercase();
    sqlx::query("INSERT OR REPLACE INTO number_sequences (name, prefix, current_value, padding) VALUES (?, ?, 0, ?)")
        .bind(&name)
        .bind(&prefix)
        .bind(padding)
        .execute(&w.pool)
        .await
        .expect("failed to insert sequence");
    w.last_seeded_name = Some(name);
}

#[given(expr = "a quote sequence is seeded with prefix {string} and padding {int}")]
async fn quote_sequence_seeded(w: &mut DbWorld, prefix: String, padding: i32) {
    sqlx::query("INSERT INTO number_sequences (name, prefix, current_value, padding) VALUES ('quote', ?, 0, ?)")
        .bind(&prefix)
        .bind(padding)
        .execute(&w.pool)
        .await
        .expect("failed to insert quote sequence");
}

#[given(expr = "{int} customer numbers have already been generated")]
async fn n_customer_numbers_generated(w: &mut DbWorld, count: i32) {
    sqlx::query("UPDATE number_sequences SET current_value = ? WHERE name = 'customer'")
        .bind(count)
        .execute(&w.pool)
        .await
        .expect("failed to advance sequence");
}

// ---- When steps ----

#[when("the next customer number is requested")]
async fn next_customer_number(w: &mut DbWorld) {
    w.result = Some(w.generator.next_value("customer").await);
}

#[when("the next number is requested")]
async fn next_number(w: &mut DbWorld) {
    let name = w
        .last_seeded_name
        .as_deref()
        .expect("no sequence seeded — use a Given step first");
    w.result = Some(w.generator.next_value(name).await);
}

#[when("the next quote number is requested")]
async fn next_quote_number(w: &mut DbWorld) {
    w.result = Some(w.generator.next_value("quote").await);
}

#[when(expr = "the next number is requested for a sequence named {string}")]
async fn next_number_for_sequence(w: &mut DbWorld, name: String) {
    w.result = Some(w.generator.next_value(&name).await);
}

#[when(expr = "{int} customer numbers are requested simultaneously")]
async fn concurrent_customer_numbers(w: &mut DbWorld, count: i32) {
    let mut join_set = tokio::task::JoinSet::new();
    for _ in 0..count {
        let pool = w.pool.clone();
        join_set.spawn(async move {
            let generator = SqliteSequenceGenerator::new(pool);
            generator.next_value("customer").await
        });
    }
    while let Some(res) = join_set.join_next().await {
        let value: Result<FormattedSequence, DomainError> = res.expect("task panicked");
        w.results.push(value);
    }
}

// ---- Then steps ----

#[then(expr = "the result is {string}")]
async fn result_is(w: &mut DbWorld, expected: String) {
    let result = w.result.as_ref().expect("no result");
    let formatted = result.as_ref().expect("expected Ok result");
    assert_eq!(
        formatted.formatted, expected,
        "Expected '{}', got '{}'",
        expected, formatted.formatted
    );
}

#[then(expr = "a {string} error is returned")]
async fn error_returned(w: &mut DbWorld, error_type: String) {
    let result = w.result.as_ref().expect("no result");
    match result {
        Err(DomainError::NotFound { .. }) if error_type == "not found" => {}
        other => panic!("Expected '{}' error, got: {:?}", error_type, other),
    }
}

#[then(expr = "all {int} results are unique")]
async fn all_results_unique(w: &mut DbWorld, count: i32) {
    let ok_results: Vec<&FormattedSequence> = w
        .results
        .iter()
        .map(|r| r.as_ref().expect("expected Ok result"))
        .collect();
    assert_eq!(ok_results.len(), count as usize);
    let unique: HashSet<&str> = ok_results.iter().map(|r| r.formatted.as_str()).collect();
    assert_eq!(
        unique.len(),
        count as usize,
        "Expected {} unique results, got {}",
        count,
        unique.len()
    );
}

#[then(expr = "the results are {string} through {string}")]
async fn results_are_range(w: &mut DbWorld, from: String, to: String) {
    let mut formatted: Vec<String> = w
        .results
        .iter()
        .map(|r| r.as_ref().expect("expected Ok result").formatted.clone())
        .collect();
    formatted.sort();
    assert_eq!(
        formatted.first().unwrap(),
        &from,
        "First result should be '{}'",
        from
    );
    assert_eq!(
        formatted.last().unwrap(),
        &to,
        "Last result should be '{}'",
        to
    );
}
