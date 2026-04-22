use crate::app_handle::AppHandleShim;
use crate::data_plane::{DataPlaneConfig, DeploymentMode};
use crate::graft::SubGraft;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// A single rate-limiter specification: max attempts within a sliding window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateWindow {
    pub max_attempts: usize,
    pub window: Duration,
}

/// Rate-limit configuration for all control-plane limiters.
///
/// Carried in [`BootConfig`] so the engine can construct limiters
/// without reaching into domain code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Login attempts — 10 per 15 min per email (LAN-mode policy).
    pub login: RateWindow,
    /// Recovery code verification — 5 per 15 min per email.
    pub recovery: RateWindow,
    /// Recovery code regeneration — 3 per hour per user.
    pub regen: RateWindow,
    /// Profile switch attempts — 3 per 15 min per user.
    pub profile_switch: RateWindow,
    /// Restore attempts — 5 per hour (shared across validate + restore).
    pub restore: RateWindow,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        let fifteen_min = Duration::from_secs(15 * 60);
        let one_hour = Duration::from_secs(3600);

        Self {
            login: RateWindow {
                max_attempts: 10,
                window: fifteen_min,
            },
            recovery: RateWindow {
                max_attempts: 5,
                window: fifteen_min,
            },
            regen: RateWindow {
                max_attempts: 3,
                window: one_hour,
            },
            profile_switch: RateWindow {
                max_attempts: 3,
                window: fifteen_min,
            },
            restore: RateWindow {
                max_attempts: 5,
                window: one_hour,
            },
        }
    }
}

pub struct BootConfig {
    pub data_dir: PathBuf,
    pub data_plane: DataPlaneConfig,
    pub rate_limit_config: RateLimitConfig,
    pub(crate) subgrafts: Vec<Box<dyn SubGraft>>,
    pub(crate) app_handle: Option<Box<dyn AppHandleShim>>,
}

impl BootConfig {
    pub fn headless_from_args() -> Result<Self, crate::error::EngineError> {
        let data_dir = std::env::var("MOKUMO_DATA_DIR")
            .map(PathBuf::from)
            .map_err(|_| {
                crate::error::EngineError::Boot(
                    "MOKUMO_DATA_DIR environment variable not set".to_string(),
                )
            })?;

        let default_addr: SocketAddr = default_bind_addr();
        let bind_addr = match std::env::args().skip_while(|a| a != "--bind-addr").nth(1) {
            Some(s) => s.parse().map_err(|_| {
                crate::error::EngineError::Boot(format!("invalid --bind-addr value: {s}"))
            })?,
            None => default_addr,
        };

        Ok(Self {
            data_dir,
            data_plane: DataPlaneConfig::lan_default(bind_addr),
            rate_limit_config: RateLimitConfig::default(),
            subgrafts: Vec::new(),
            app_handle: None,
        })
    }

    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            data_plane: DataPlaneConfig::lan_default(default_bind_addr()),
            rate_limit_config: RateLimitConfig::default(),
            subgrafts: Vec::new(),
            app_handle: None,
        }
    }

    pub fn with_subgraft(mut self, sg: impl SubGraft + 'static) -> Self {
        self.subgrafts.push(Box::new(sg));
        self
    }

    /// Replace the full [`DataPlaneConfig`]. Prefer this over the individual
    /// `with_deployment_mode` / `with_bind_addr` shortcuts when the caller
    /// owns the allowed-hosts / allowed-origins lists.
    pub fn with_data_plane(mut self, cfg: DataPlaneConfig) -> Self {
        self.data_plane = cfg;
        self
    }

    /// Shortcut: set only the deployment mode, leave other data-plane fields
    /// unchanged.
    pub fn with_deployment_mode(mut self, mode: DeploymentMode) -> Self {
        self.data_plane.deployment_mode = mode;
        self
    }

    /// Shortcut: set only the bind address, leave other data-plane fields
    /// unchanged.
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.data_plane.bind_addr = addr;
        self
    }

    pub fn with_rate_limit_config(mut self, config: RateLimitConfig) -> Self {
        self.rate_limit_config = config;
        self
    }

    /// Deployment mode selected for this boot. Shortcut over
    /// `self.data_plane.deployment_mode`.
    pub fn deployment_mode(&self) -> DeploymentMode {
        self.data_plane.deployment_mode
    }

    /// Bind address selected for this boot.
    pub fn bind_addr(&self) -> SocketAddr {
        self.data_plane.bind_addr
    }

    pub fn tauri_desktop(handle: impl AppHandleShim + 'static) -> Self {
        let data_dir = handle.data_dir().expect("AppHandle must provide data_dir");
        Self {
            data_dir,
            data_plane: DataPlaneConfig::lan_default(default_bind_addr()),
            rate_limit_config: RateLimitConfig::default(),
            subgrafts: Vec::new(),
            app_handle: Some(Box::new(handle)),
        }
    }
}

fn default_bind_addr() -> SocketAddr {
    "127.0.0.1:3000"
        .parse()
        .expect("static `127.0.0.1:3000` parses as SocketAddr")
}

impl std::fmt::Debug for BootConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootConfig")
            .field("data_dir", &self.data_dir)
            .field("data_plane", &self.data_plane)
            .field("rate_limit_config", &self.rate_limit_config)
            .field("subgraft_count", &self.subgrafts.len())
            .field("has_app_handle", &self.app_handle.is_some())
            .finish()
    }
}
