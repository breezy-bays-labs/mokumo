//! Sequence vertical — atomic human-readable sequential identifiers.
//!
//! Provides the `SequenceGenerator` port (for CustomerNumber, QuoteNumber,
//! InvoiceNumber generation) and a SQLite adapter backed by the
//! `number_sequences` table. Neutral to decorated-apparel semantics —
//! every shop vertical that needs business-visible sequential IDs shares
//! the same core.

pub mod adapter;
pub mod domain;

pub use adapter::SqliteSequenceGenerator;
pub use domain::{FormattedSequence, format_sequence_number};

use kikan::error::DomainError;

/// Port for atomic sequence number generation.
///
/// Implementors must guarantee that concurrent calls never produce
/// duplicate values for the same sequence name.
pub trait SequenceGenerator: Send + Sync {
    fn next_value(
        &self,
        sequence_name: &str,
    ) -> impl Future<Output = Result<FormattedSequence, DomainError>> + Send;
}
