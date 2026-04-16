use std::path::Path;
use std::time::{Duration, SystemTime};

use apalis::prelude::*;
use apalis_sqlite::SqliteStorage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::error::SchedulerError;
use crate::job::JobId;
use crate::scheduler::Scheduler;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GenericJob {
    payload_name: String,
    payload_json: serde_json::Value,
}

pub struct ApalisScheduler {
    pool: SqlitePool,
    storage: Mutex<Box<dyn TaskSinkDyn<GenericJob>>>,
    shutdown: CancellationToken,
}

trait TaskSinkDyn<T>: Send {
    fn push_job(
        &mut self,
        task: T,
        run_at: SystemTime,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>>;
}

impl<S> TaskSinkDyn<GenericJob> for S
where
    S: TaskSink<GenericJob> + Send,
    S::Error: std::fmt::Display,
    S::Context: Default,
    S::IdType: Default,
{
    fn push_job(
        &mut self,
        job: GenericJob,
        run_at: SystemTime,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(async move {
            let task = Task::builder(job).run_at_time(run_at).build();
            self.push_task(task).await.map_err(|e| e.to_string())
        })
    }
}

impl ApalisScheduler {
    pub async fn new(db_path: &Path) -> Result<Self, SchedulerError> {
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&url)
            .await
            .map_err(|e| SchedulerError::Storage(e.to_string()))?;

        SqliteStorage::setup(&pool)
            .await
            .map_err(|e| SchedulerError::Storage(e.to_string()))?;

        let storage = SqliteStorage::new(&pool);

        Ok(Self {
            pool,
            storage: Mutex::new(Box::new(storage)),
            shutdown: CancellationToken::new(),
        })
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl std::fmt::Debug for ApalisScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApalisScheduler").finish_non_exhaustive()
    }
}

#[async_trait]
impl Scheduler for ApalisScheduler {
    async fn schedule_after(
        &self,
        payload_name: &str,
        delay: Duration,
        payload_json: serde_json::Value,
    ) -> Result<JobId, SchedulerError> {
        let job = GenericJob {
            payload_name: payload_name.to_string(),
            payload_json,
        };

        let run_at = SystemTime::now() + delay;

        let mut storage = self.storage.lock().await;
        storage
            .push_job(job, run_at)
            .await
            .map_err(SchedulerError::Storage)?;

        Ok(JobId::new(format!(
            "{payload_name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )))
    }

    async fn cancel(&self, _id: &JobId) -> Result<(), SchedulerError> {
        Ok(())
    }

    async fn check(&self) -> Result<(), SchedulerError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| SchedulerError::Storage(e.to_string()))?;
        Ok(())
    }
}
