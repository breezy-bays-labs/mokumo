use crate::error::DomainError;
use crate::pagination::PageParams;
use kikan_types::activity::ActivityEntry;

/// Port for activity log read operations.
///
/// Write operations (inserting activity entries) are handled internally
/// by entity repository adapters within their mutation transactions.
pub trait ActivityLogRepository: Send + Sync {
    fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        params: PageParams,
    ) -> impl Future<Output = Result<(Vec<ActivityEntry>, i64), DomainError>> + Send;
}
