use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("storage error: {0}")]
    Storage(String),

    #[error("no handler registered for payload name {0}")]
    NoHandler(String),

    #[error("invalid cron expression: {0}")]
    InvalidCron(String),

    #[error("failed to serialize job payload: {0}")]
    SerializeFailed(String),

    #[error("worker pool is unhealthy")]
    WorkerPoolUnhealthy,

    #[error("job execution failed: {0}")]
    JobFailed(String),
}
