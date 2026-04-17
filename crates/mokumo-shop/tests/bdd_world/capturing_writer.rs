//! Test double for `kikan::ActivityWriter` that records each call.
//!
//! Captures the raw address of the `&DatabaseTransaction` reference (as
//! `usize`) alongside the `ActivityLogEntry` on each call. Supports a
//! "fail on next call" mode to exercise the atomicity rollback paths.
//!
//! When `persist = true`, the writer also forwards the insert to the real
//! `kikan::SqliteActivityWriter` so the activity_log row actually lands —
//! the customer_atomicity scenarios assert on activity_log contents.

use std::sync::Mutex;

use async_trait::async_trait;
use kikan::activity::{ActivityLogEntry, ActivityWriter, SqliteActivityWriter};
use kikan::error::ActivityWriteError;
use sea_orm::DatabaseTransaction;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CapturedCall {
    pub tx_addr: usize,
    pub actor_id: Option<String>,
    pub actor_type: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub action: String,
    pub payload: serde_json::Value,
}

pub struct CapturingActivityWriter {
    inner: SqliteActivityWriter,
    calls: Mutex<Vec<CapturedCall>>,
    fail_next: Mutex<bool>,
    persist: bool,
}

impl CapturingActivityWriter {
    pub fn new_persisting() -> Self {
        Self {
            inner: SqliteActivityWriter::new(),
            calls: Mutex::new(Vec::new()),
            fail_next: Mutex::new(false),
            persist: true,
        }
    }

    pub fn arm_failure(&self) {
        *self.fail_next.lock().unwrap() = true;
    }

    pub fn calls(&self) -> Vec<CapturedCall> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl ActivityWriter for CapturingActivityWriter {
    async fn log(
        &self,
        tx: &DatabaseTransaction,
        entry: ActivityLogEntry,
    ) -> Result<(), ActivityWriteError> {
        let tx_addr = tx as *const DatabaseTransaction as usize;
        self.calls.lock().unwrap().push(CapturedCall {
            tx_addr,
            actor_id: entry.actor_id.clone(),
            actor_type: entry.actor_type.clone(),
            entity_kind: entry.entity_kind.clone(),
            entity_id: entry.entity_id.clone(),
            action: entry.action.clone(),
            payload: entry.payload.clone(),
        });

        if std::mem::replace(&mut *self.fail_next.lock().unwrap(), false) {
            return Err(ActivityWriteError::Db(sea_orm::DbErr::Custom(
                "armed failure from CapturingActivityWriter".into(),
            )));
        }

        if self.persist {
            self.inner.log(tx, entry).await?;
        }
        Ok(())
    }
}
