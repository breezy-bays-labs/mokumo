//! Kikan job scheduler SubGraft — `apalis` (SQLite-backed) for
//! production plus an [`ImmediateScheduler`] for tests.
//!
//! Composes into a [`kikan::Engine`] via [`SchedulerSubGraft`] (a
//! [`kikan::SubGraft`] impl). Schedule a typed payload with
//! [`schedule_after_typed`]; implement [`Scheduler`] to back-end-swap
//! (e.g. an in-memory queue for unit tests). Depends on `kikan` for
//! the SubGraft trait surface only.

pub mod apalis_impl;
pub mod error;
pub mod immediate;
pub mod job;
pub mod scheduler;
pub mod subgraft;

pub use apalis_impl::ApalisScheduler;
pub use error::SchedulerError;
pub use immediate::ImmediateScheduler;
pub use job::{JobId, JobPayload};
pub use scheduler::{Scheduler, schedule_after_typed};
pub use subgraft::SchedulerSubGraft;
