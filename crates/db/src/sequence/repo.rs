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
