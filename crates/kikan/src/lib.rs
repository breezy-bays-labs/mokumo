pub mod activity;
pub mod app_error;
pub mod app_handle;
pub mod auth;
pub mod backup;
pub mod boot;
pub mod control_plane;
pub mod control_plane_error;
pub mod db;
pub mod engine;
pub mod error;
pub mod graft;
pub mod logging;
pub mod middleware;
pub mod migrations;
pub mod platform;
pub mod platform_state;
pub mod profile_db;
pub mod rate_limit;
pub mod tenancy;

pub use activity::{ActivityLogEntry, ActivityWriter, SqliteActivityWriter};
pub use app_error::AppError;
pub use app_handle::AppHandleShim;
pub use boot::{BootConfig, DeploymentMode, RateLimitConfig, RateWindow};
pub use control_plane::{ControlPlaneState, PendingReset};
pub use control_plane_error::{ConflictKind, ControlPlaneError};
pub mod admin {
    pub use crate::engine::admin::build_admin_router;
}
pub use engine::{
    Engine, EngineContext, Sessions, platform_protected_routes, platform_public_routes,
};
pub use error::{
    ActivityWriteError, AppHandleError, DagError, EngineError, MigrationError, TenancyError,
};
pub use graft::{Graft, SelfGraft, SubGraft};
pub use migrations::{GraftId, Migration, MigrationRef, MigrationTarget};
pub use platform_state::{MdnsStatus, PlatformState, SharedMdnsStatus};
pub use profile_db::{ActiveProfile, ProfileDb};
pub use tenancy::{ProfileId, SetupMode, Tenancy};
