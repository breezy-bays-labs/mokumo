use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventBusError {
    #[error("subscriber lagged {0} events behind the channel head")]
    Lagged(u64),
}
