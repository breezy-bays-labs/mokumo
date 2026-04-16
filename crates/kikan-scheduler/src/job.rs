use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    pub(crate) fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn get(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait JobPayload: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static {
    const NAME: &'static str;
}
