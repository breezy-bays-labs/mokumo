//! Shop vertical — settings and logo management.
//!
//! Owns the `shop_settings` singleton: the stored logo extension + epoch
//! cache-buster, plus the activity-log writes that accompany upload /
//! removal. The production-profile guard lives at the handler layer and
//! the rate-limiter is a router-deps singleton owned by the shell.

pub mod adapter;
pub mod domain;
pub mod entity;
pub mod error;
pub mod handler;
pub mod logo_validator;
pub mod service;

pub use adapter::SqliteShopLogoRepository;
pub use domain::ShopLogoInfo;
pub use error::ShopLogoHandlerError;
pub use handler::{ShopLogoRouterDeps, shop_logo_protected_router, shop_logo_public_router};
pub use logo_validator::{LogoError, LogoFormat, LogoValidator, ValidatedLogo};
pub use service::ShopLogoService;

use kikan::actor::Actor;
use kikan::error::DomainError;

/// Port for shop-logo persistence.
///
/// Mirrors `CustomerRepository`: trait lives here, SQLite implementation
/// in `adapter.rs`, and the service takes the trait so tests can swap in
/// a fault-injecting double for atomicity scenarios.
pub trait ShopLogoRepository: Send + Sync {
    fn get_logo_info(
        &self,
    ) -> impl Future<Output = Result<Option<ShopLogoInfo>, DomainError>> + Send;

    fn upsert_logo(
        &self,
        extension: &str,
        updated_at: i64,
        actor: &Actor,
    ) -> impl Future<Output = Result<(), DomainError>> + Send;

    fn delete_logo(&self, actor: &Actor) -> impl Future<Output = Result<(), DomainError>> + Send;
}
