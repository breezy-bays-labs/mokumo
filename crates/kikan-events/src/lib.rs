//! Kikan event bus SubGraft — typed `tokio::sync::broadcast` wrapper.
//!
//! Two layers:
//! - [`channel::FanoutChannel<T>`] — domain-neutral broadcast mechanism.
//!   Reusable by any caller that wants typed fanout.
//! - [`bus::BroadcastEventBus`] — kikan platform-event taxonomy composing
//!   four `FanoutChannel<T>`s internally. Registered via
//!   [`EventBusSubGraft`] (a [`kikan::SubGraft`] impl); publishes on Engine
//!   lifecycle hooks. Depends on `kikan` for the SubGraft trait surface
//!   only — never the reverse direction (invariant I4).

pub mod bus;
pub mod channel;
pub mod error;
pub mod event;
pub mod subgraft;

pub use bus::BroadcastEventBus;
pub use channel::{DEFAULT_CAPACITY, FanoutChannel};
pub use error::EventBusError;
pub use event::{Event, HealthEvent, LifecycleEvent, MigrationEvent, ProfileEvent};
pub use subgraft::EventBusSubGraft;
