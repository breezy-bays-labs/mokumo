use kikan::{AppHandleError, AppHandleShim};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct StubAppHandle {
    data_dir: PathBuf,
    events: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
}

impl StubAppHandle {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn events(&self) -> Vec<(String, serde_json::Value)> {
        self.events.lock().unwrap().clone()
    }
}

impl AppHandleShim for StubAppHandle {
    fn data_dir(&self) -> Result<PathBuf, AppHandleError> {
        Ok(self.data_dir.clone())
    }

    fn app_version(&self) -> &'static str {
        "0.1.0-test"
    }

    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), payload));
    }
}
