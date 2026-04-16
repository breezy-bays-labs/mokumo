use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;

use crate::error::SchedulerError;
use crate::job::JobId;
use crate::scheduler::Scheduler;

type BoxedHandler = Box<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = Result<(), SchedulerError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone)]
pub struct PendingJob {
    pub id: JobId,
    pub payload_name: String,
    pub delay: Duration,
    pub payload_json: serde_json::Value,
}

pub struct ImmediateScheduler {
    pending: Arc<Mutex<Vec<PendingJob>>>,
    handlers: Arc<RwLock<HashMap<String, Arc<BoxedHandler>>>>,
    next_id: Arc<Mutex<u64>>,
}

impl Default for ImmediateScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ImmediateScheduler {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(Vec::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
        }
    }

    pub fn register_handler<F, Fut>(&self, payload_name: impl Into<String>, handler: F)
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), SchedulerError>> + Send + 'static,
    {
        let handler: BoxedHandler = Box::new(move |v| Box::pin(handler(v)));
        self.handlers
            .write()
            .unwrap()
            .insert(payload_name.into(), Arc::new(handler));
    }

    pub fn pending_jobs(&self) -> Vec<PendingJob> {
        self.pending.lock().unwrap().clone()
    }

    fn next_job_id(&self) -> JobId {
        let mut id = self.next_id.lock().unwrap();
        let job_id = JobId::new(format!("immediate-{id}"));
        *id += 1;
        job_id
    }
}

impl std::fmt::Debug for ImmediateScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImmediateScheduler")
            .field("pending", &self.pending.lock().unwrap().len())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Scheduler for ImmediateScheduler {
    async fn schedule_after(
        &self,
        payload_name: &str,
        delay: Duration,
        payload_json: serde_json::Value,
    ) -> Result<JobId, SchedulerError> {
        let id = self.next_job_id();

        let handler = {
            let handlers = self.handlers.read().unwrap();
            handlers
                .get(payload_name)
                .ok_or_else(|| SchedulerError::NoHandler(payload_name.to_string()))?
                .clone()
        };

        if delay.is_zero() {
            handler(payload_json).await?;
            return Ok(id);
        }

        self.pending.lock().unwrap().push(PendingJob {
            id: id.clone(),
            payload_name: payload_name.to_string(),
            delay,
            payload_json,
        });
        Ok(id)
    }

    async fn cancel(&self, id: &JobId) -> Result<(), SchedulerError> {
        self.pending.lock().unwrap().retain(|j| j.id != *id);
        Ok(())
    }

    async fn check(&self) -> Result<(), SchedulerError> {
        Ok(())
    }
}
