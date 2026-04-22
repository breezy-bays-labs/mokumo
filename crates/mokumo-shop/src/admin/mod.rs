//! Mokumo-specific admin surface — UDS router + HTTP diagnostics shells
//! + per-profile list / switch wire-DTO wrappers.
//!
//! This module holds the Mokumo-vocabulary portions of the admin surface:
//! `SetupMode`-typed DTOs (`ProfileListResponse`, `DiagnosticsResponse`,
//! `ProfileSwitchAdminResponse`), Mokumo-branded zip filenames, and the
//! `mokumo*.log` log-pattern filter. The generic pieces (disk warnings,
//! per-profile DB diagnostics, sysinfo, redaction) live here too —
//! kikan does not need them for anything but admin.
//!
//! The UDS router built by [`build_admin_router`] is served by
//! `mokumo-server` over a Unix domain socket with mode `0600` as the
//! capability-based admin channel.

pub mod diagnostics;
pub mod diagnostics_bundle_http;
pub mod diagnostics_http;
pub mod profile_list;
pub mod profile_switch;
pub mod router;

pub use router::build_admin_router;
