//! Kikan event bus SubGraft — typed `tokio::sync::broadcast` wrapper.
//!
//! Composes into a [`kikan::Engine`] via [`EventBusSubGraft`] (a
//! [`kikan::SubGraft`] impl). Add a new event variant in [`event`],
//! publish via [`BroadcastEventBus::publish`], and subscribe via
//! [`BroadcastEventBus::subscribe`]. Depends on `kikan` for the
//! SubGraft trait surface only — never the reverse direction
//! (invariant I4).

pub mod bus;
pub mod error;
pub mod event;
pub mod subgraft;

pub use bus::{BroadcastEventBus, DEFAULT_CAPACITY};
pub use error::EventBusError;
pub use event::{Event, HealthEvent, LifecycleEvent, MigrationEvent, ProfileEvent};
pub use subgraft::EventBusSubGraft;
