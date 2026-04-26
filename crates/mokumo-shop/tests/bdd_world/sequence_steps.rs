//! Step definitions for `number_sequences.feature` and
//! `sequence_formatting.feature`. Drives the real
//! `SqliteSequenceGenerator` against a tempdir-backed SQLite pool.

use std::collections::HashSet;

use cucumber::{given, then, when};
use kikan::error::DomainError;
use mokumo_shop::sequence::{
    FormattedSequence, SequenceGenerator, SqliteSequenceGenerator, format_sequence_number,
};
use sqlx::SqlitePool;

use super::MokumoShopWorld;

async fn ensure_pool(w: &mut MokumoShopWorld) -> SqlitePool {
    if w.db.is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = mokumo_shop::db::initialize_database(&url).await.unwrap();
        w.db = Some(db);
        w._tmp = Some(tmp);
    }
    w.db.as_ref().unwrap().get_sqlite_connection_pool().clone()
}

fn generator(w: &MokumoShopWorld) -> SqliteSequenceGenerator {
    SqliteSequenceGenerator::new(w.db.as_ref().unwrap().get_sqlite_connection_pool().clone())
}

// --- sequence_formatting.feature ---------------------------------------

#[given(expr = "a prefix {string} and padding {int}")]
async fn given_prefix_and_padding(w: &mut MokumoShopWorld, prefix: String, padding: i32) {
    w.seq_prefix = prefix;
    w.seq_padding = padding as u32;
}

#[when(expr = "value {int} is formatted")]
async fn when_value_formatted(w: &mut MokumoShopWorld, value: i64) {
    w.seq_format_result = format_sequence_number(&w.seq_prefix, value, w.seq_padding);
}

#[then(expr = "the display number is {string}")]
async fn then_display_number_is(w: &mut MokumoShopWorld, expected: String) {
    assert_eq!(w.seq_format_result, expected);
}

// --- number_sequences.feature ------------------------------------------

#[given(expr = "the customer sequence is seeded with prefix {string} and padding {int}")]
async fn customer_sequence_seeded(w: &mut MokumoShopWorld, prefix: String, padding: i32) {
    let pool = ensure_pool(w).await;
    let row = sqlx::query_as::<_, (String, i64)>(
        "SELECT prefix, padding FROM number_sequences WHERE name = 'customer'",
    )
    .fetch_one(&pool)
    .await
    .expect("customer sequence must be seeded by migration");
    assert_eq!(row.0, prefix);
    assert_eq!(row.1, padding as i64);
}

#[given(expr = "a sequence with prefix {string} and padding {int}")]
async fn sequence_with_prefix(w: &mut MokumoShopWorld, prefix: String, padding: i32) {
    let pool = ensure_pool(w).await;
    let name = prefix.to_lowercase();
    sqlx::query(
        "INSERT OR REPLACE INTO number_sequences (name, prefix, current_value, padding) \
         VALUES (?, ?, 0, ?)",
    )
    .bind(&name)
    .bind(&prefix)
    .bind(padding)
    .execute(&pool)
    .await
    .expect("insert sequence");
    w.seq_last_seeded_name = Some(name);
}

#[given(expr = "a quote sequence is seeded with prefix {string} and padding {int}")]
async fn quote_sequence_seeded(w: &mut MokumoShopWorld, prefix: String, padding: i32) {
    let pool = ensure_pool(w).await;
    sqlx::query(
        "INSERT INTO number_sequences (name, prefix, current_value, padding) \
         VALUES ('quote', ?, 0, ?)",
    )
    .bind(&prefix)
    .bind(padding)
    .execute(&pool)
    .await
    .expect("insert quote sequence");
}

#[given(expr = "{int} customer numbers have already been generated")]
async fn n_customer_numbers_generated(w: &mut MokumoShopWorld, count: i32) {
    let pool = ensure_pool(w).await;
    sqlx::query("UPDATE number_sequences SET current_value = ? WHERE name = 'customer'")
        .bind(count)
        .execute(&pool)
        .await
        .expect("advance sequence");
}

#[when("the next customer number is requested")]
async fn next_customer_number(w: &mut MokumoShopWorld) {
    ensure_pool(w).await;
    w.seq_result = Some(generator(w).next_value("customer").await);
}

#[when("the next number is requested")]
async fn next_number(w: &mut MokumoShopWorld) {
    ensure_pool(w).await;
    let name = w.seq_last_seeded_name.clone().expect("no sequence seeded");
    w.seq_result = Some(generator(w).next_value(&name).await);
}

#[when("the next quote number is requested")]
async fn next_quote_number(w: &mut MokumoShopWorld) {
    ensure_pool(w).await;
    w.seq_result = Some(generator(w).next_value("quote").await);
}

#[when(expr = "the next number is requested for a sequence named {string}")]
async fn next_number_for_sequence(w: &mut MokumoShopWorld, name: String) {
    ensure_pool(w).await;
    w.seq_result = Some(generator(w).next_value(&name).await);
}

#[when(expr = "{int} customer numbers are requested simultaneously")]
async fn concurrent_customer_numbers(w: &mut MokumoShopWorld, count: i32) {
    let pool = ensure_pool(w).await;
    let mut join_set = tokio::task::JoinSet::new();
    for _ in 0..count {
        let pool = pool.clone();
        join_set.spawn(async move {
            let generator = SqliteSequenceGenerator::new(pool);
            generator.next_value("customer").await
        });
    }
    while let Some(res) = join_set.join_next().await {
        w.seq_results.push(res.expect("task panicked"));
    }
}

#[then(expr = "the result is {string}")]
async fn result_is(w: &mut MokumoShopWorld, expected: String) {
    let formatted = w
        .seq_result
        .as_ref()
        .expect("no result")
        .as_ref()
        .expect("expected Ok");
    assert_eq!(formatted.formatted, expected);
}

#[then(expr = "a {string} error is returned")]
async fn error_returned(w: &mut MokumoShopWorld, error_type: String) {
    let result = w.seq_result.as_ref().expect("no result");
    match result {
        Err(DomainError::NotFound { .. }) if error_type == "not found" => {}
        other => panic!("expected '{error_type}', got: {other:?}"),
    }
}

#[then(expr = "all {int} results are unique")]
async fn all_results_unique(w: &mut MokumoShopWorld, count: i32) {
    let ok_results: Vec<&FormattedSequence> = w
        .seq_results
        .iter()
        .map(|r| r.as_ref().expect("Ok"))
        .collect();
    assert_eq!(ok_results.len(), count as usize);
    let unique: HashSet<&str> = ok_results.iter().map(|r| r.formatted.as_str()).collect();
    assert_eq!(unique.len(), count as usize);
}

#[then(expr = "the results are {string} through {string}")]
async fn results_are_range(w: &mut MokumoShopWorld, from: String, to: String) {
    let mut formatted: Vec<String> = w
        .seq_results
        .iter()
        .map(|r| r.as_ref().expect("Ok").formatted.clone())
        .collect();
    formatted.sort();
    assert_eq!(formatted.first().unwrap(), &from);
    assert_eq!(formatted.last().unwrap(), &to);
}
