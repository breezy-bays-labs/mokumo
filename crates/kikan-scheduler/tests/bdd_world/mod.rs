use std::sync::{Arc, Mutex};
use std::time::Duration;

use cucumber::{World, given, then, when};

use kikan_scheduler::{ImmediateScheduler, JobId, Scheduler, SchedulerError};

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct SchedulerWorld {
    scheduler: Arc<ImmediateScheduler>,
    counter: Arc<Mutex<i64>>,
    log: Arc<Mutex<String>>,
    last_job_id: Option<JobId>,
    last_error: Option<SchedulerError>,
}

impl SchedulerWorld {
    fn new() -> Self {
        let scheduler = Arc::new(ImmediateScheduler::new());
        let counter = Arc::new(Mutex::new(0i64));
        let log = Arc::new(Mutex::new(String::new()));

        {
            let counter = counter.clone();
            scheduler.register_handler("counter", move |_v: serde_json::Value| {
                let counter = counter.clone();
                async move {
                    *counter.lock().unwrap() += 1;
                    Ok(())
                }
            });
        }
        {
            let log = log.clone();
            scheduler.register_handler("log-a", move |_v: serde_json::Value| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push('a');
                    Ok(())
                }
            });
        }
        {
            let log = log.clone();
            scheduler.register_handler("log-b", move |_v: serde_json::Value| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push('b');
                    Ok(())
                }
            });
        }

        Self {
            scheduler,
            counter,
            log,
            last_job_id: None,
            last_error: None,
        }
    }
}

// --- Background ---

#[given("an ImmediateScheduler instance")]
async fn given_scheduler(w: &mut SchedulerWorld) {
    *w = SchedulerWorld::new();
}

#[given("a counter initialized to 0")]
async fn given_counter(w: &mut SchedulerWorld) {
    *w.counter.lock().unwrap() = 0;
}

// --- When steps ---

#[when("a zero-delay job that increments the counter is scheduled")]
async fn schedule_zero_counter(w: &mut SchedulerWorld) {
    let result = w
        .scheduler
        .schedule_after("counter", Duration::ZERO, serde_json::json!({}))
        .await;
    match result {
        Ok(id) => w.last_job_id = Some(id),
        Err(e) => w.last_error = Some(e),
    }
}

#[when("a deferred job with 60s delay is scheduled")]
async fn schedule_deferred(w: &mut SchedulerWorld) {
    let result = w
        .scheduler
        .schedule_after("counter", Duration::from_secs(60), serde_json::json!({}))
        .await;
    match result {
        Ok(id) => w.last_job_id = Some(id),
        Err(e) => w.last_error = Some(e),
    }
}

#[when(expr = "an inline job appending {string} to the log is scheduled")]
async fn schedule_log_append(w: &mut SchedulerWorld, letter: String) {
    let handler_name = format!("log-{letter}");
    w.scheduler
        .schedule_after(&handler_name, Duration::ZERO, serde_json::json!({}))
        .await
        .unwrap();
}

// --- Then steps ---

#[then("the counter equals 1 immediately after the schedule call returns")]
async fn then_counter_is_1(w: &mut SchedulerWorld) {
    assert_eq!(*w.counter.lock().unwrap(), 1);
}

#[then("the job has not executed")]
async fn then_job_not_executed(w: &mut SchedulerWorld) {
    assert_eq!(*w.counter.lock().unwrap(), 0);
}

#[then("the scheduler reports the job as pending")]
async fn then_job_pending(w: &mut SchedulerWorld) {
    let pending = w.scheduler.pending_jobs();
    assert!(!pending.is_empty(), "expected at least one pending job");
}

#[then(expr = "the log equals {string}")]
async fn then_log_equals(w: &mut SchedulerWorld, expected: String) {
    assert_eq!(*w.log.lock().unwrap(), expected);
}
