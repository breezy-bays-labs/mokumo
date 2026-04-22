//! Platform-side request handlers and infrastructure modules.
//!
//! Every module here takes the kikan-owned [`PlatformState`](crate::PlatformState)
//! (or a finer-grained slice like [`SharedMdnsStatus`](crate::SharedMdnsStatus))
//! rather than the vertical's `AppState`, so kikan stays I4-clean.
//!
//! - [`backup_status`] — `GET /api/backup-status`
//! - [`discovery`] — mDNS registration helpers (no router)
//! - [`activity_http`] — `GET /api/activity` list endpoint

pub mod activity_http;
pub mod backup_status;
pub mod discovery;
