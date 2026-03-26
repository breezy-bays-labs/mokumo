use crate::activity::ActivityEntry;
use crate::error::DomainError;
use crate::pagination::PageParams;

/// Port for activity log read operations.
///
/// Write operations (inserting activity entries) are handled internally
/// by entity repository adapters within their mutation transactions.
/// See `crates/db/src/activity/repo::insert_activity_log_raw`.
pub trait ActivityLogRepository: Send + Sync {
    fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        params: PageParams,
    ) -> impl Future<Output = Result<(Vec<ActivityEntry>, i64), DomainError>> + Send;
}
