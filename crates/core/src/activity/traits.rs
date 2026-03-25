use crate::activity::ActivityEntry;
use crate::error::DomainError;
use crate::pagination::PageParams;

/// Port for activity log persistence operations.
pub trait ActivityLogRepository: Send + Sync {
    fn log(
        &self,
        entity_type: &str,
        entity_id: &str,
        action: &str,
        actor_id: &str,
        actor_type: &str,
        payload: &serde_json::Value,
    ) -> impl Future<Output = Result<ActivityEntry, DomainError>> + Send;

    fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        params: PageParams,
    ) -> impl Future<Output = Result<(Vec<ActivityEntry>, i64), DomainError>> + Send;
}
