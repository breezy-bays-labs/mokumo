//! Shop-logo service — thin orchestration over the repository.
//!
//! The service layer is intentionally minimal: the repository enforces
//! atomicity (upsert + activity log in the same transaction) and the
//! handler owns multipart parsing, filesystem writes, and the
//! production-profile guard.

use kikan::actor::Actor;
use kikan::error::DomainError;

use crate::shop::ShopLogoRepository;
use crate::shop::domain::ShopLogoInfo;

pub struct ShopLogoService<R: ShopLogoRepository> {
    repo: R,
}

impl<R: ShopLogoRepository> ShopLogoService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn get_logo_info(&self) -> Result<Option<ShopLogoInfo>, DomainError> {
        self.repo.get_logo_info().await
    }

    pub async fn upsert_logo(
        &self,
        extension: &str,
        updated_at: i64,
        actor: &Actor,
    ) -> Result<(), DomainError> {
        self.repo.upsert_logo(extension, updated_at, actor).await
    }

    pub async fn delete_logo(&self, actor: &Actor) -> Result<(), DomainError> {
        self.repo.delete_logo(actor).await
    }
}
