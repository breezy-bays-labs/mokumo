use mokumo_core::error::DomainError;
use mokumo_core::sequence::traits::SequenceGenerator;
use mokumo_core::sequence::{FormattedSequence, format_sequence_number};
use sqlx::SqlitePool;

#[derive(Debug)]
pub struct SqliteSequenceGenerator {
    pool: SqlitePool,
}

impl SqliteSequenceGenerator {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl SequenceGenerator for SqliteSequenceGenerator {
    async fn next_value(&self, sequence_name: &str) -> Result<FormattedSequence, DomainError> {
        let row = sqlx::query_as::<_, (i64, String, i64)>(
            "UPDATE number_sequences SET current_value = current_value + 1 \
             WHERE name = ? \
             RETURNING current_value, prefix, padding",
        )
        .bind(sequence_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal {
            message: e.to_string(),
        })?;

        match row {
            Some((value, prefix, padding)) => {
                let pad = u32::try_from(padding).map_err(|_| DomainError::Internal {
                    message: format!(
                        "invalid padding value {padding} for sequence '{sequence_name}'"
                    ),
                })?;
                let formatted = format_sequence_number(&prefix, value, pad);
                Ok(FormattedSequence {
                    raw_value: value,
                    formatted,
                })
            }
            None => Err(DomainError::NotFound {
                entity: "sequence",
                id: sequence_name.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> (sqlx::SqlitePool, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = crate::initialize_database(&url).await.unwrap();
        let pool = db.get_sqlite_connection_pool().clone();
        (pool, tmp)
    }

    #[tokio::test]
    async fn next_value_increments_and_formats() {
        let (pool, _tmp) = test_pool().await;
        let seq_gen = SqliteSequenceGenerator::new(pool);
        // The "customer" sequence starts at 0 — first call returns 1
        let seq = seq_gen.next_value("customer").await.unwrap();
        assert_eq!(seq.raw_value, 1);
        assert_eq!(seq.formatted, "C-0001");
    }

    #[tokio::test]
    async fn next_value_increments_on_each_call() {
        let (pool, _tmp) = test_pool().await;
        let seq_gen = SqliteSequenceGenerator::new(pool);
        let first = seq_gen.next_value("customer").await.unwrap();
        let second = seq_gen.next_value("customer").await.unwrap();
        assert_eq!(first.raw_value, 1);
        assert_eq!(second.raw_value, 2);
        assert_eq!(second.formatted, "C-0002");
    }

    #[tokio::test]
    async fn next_value_returns_not_found_for_unknown_sequence() {
        let (pool, _tmp) = test_pool().await;
        let seq_gen = SqliteSequenceGenerator::new(pool);
        let result = seq_gen.next_value("nonexistent").await;
        assert!(
            matches!(
                result,
                Err(DomainError::NotFound {
                    entity: "sequence",
                    ..
                })
            ),
            "unknown sequence should return NotFound"
        );
    }
}
