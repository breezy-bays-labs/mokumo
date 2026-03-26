use super::FormattedSequence;
use crate::error::DomainError;

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
