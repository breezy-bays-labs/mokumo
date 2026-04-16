use std::time::Duration;

use async_trait::async_trait;

use crate::error::SchedulerError;
use crate::job::{JobId, JobPayload};

#[async_trait]
pub trait Scheduler: Send + Sync {
    async fn schedule_after(
        &self,
        payload_name: &str,
        delay: Duration,
        payload_json: serde_json::Value,
    ) -> Result<JobId, SchedulerError>;

    async fn cancel(&self, id: &JobId) -> Result<(), SchedulerError>;

    async fn check(&self) -> Result<(), SchedulerError>;
}

pub async fn schedule_after_typed<P: JobPayload>(
    scheduler: &dyn Scheduler,
    delay: Duration,
    payload: P,
) -> Result<JobId, SchedulerError> {
    let json = serde_json::to_value(&payload)
        .map_err(|e| SchedulerError::SerializeFailed(e.to_string()))?;
    scheduler.schedule_after(P::NAME, delay, json).await
}
