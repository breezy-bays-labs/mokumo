use std::collections::HashMap;

use crate::customer::traits::CustomerRepository;
use crate::customer::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use crate::error::DomainError;
use crate::filter::IncludeDeleted;
use crate::pagination::PageParams;

pub struct CustomerService<R> {
    repo: R,
}

impl<R: CustomerRepository> CustomerService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(
        &self,
        id: &CustomerId,
        filter: IncludeDeleted,
    ) -> Result<Option<Customer>, DomainError> {
        self.repo.find_by_id(id, filter).await
    }

    pub async fn list(
        &self,
        params: PageParams,
        filter: IncludeDeleted,
        search: Option<&str>,
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        self.repo.list(params, filter, search).await
    }

    pub async fn create(&self, req: &CreateCustomer) -> Result<Customer, DomainError> {
        if req.display_name.trim().is_empty() {
            return Err(DomainError::Validation {
                details: HashMap::from([(
                    "display_name".into(),
                    vec!["Display name is required".into()],
                )]),
            });
        }
        let mut normalized = req.clone();
        normalized.display_name = req.display_name.trim().to_string();
        self.repo.create(&normalized).await
    }

    pub async fn update(
        &self,
        id: &CustomerId,
        req: &UpdateCustomer,
    ) -> Result<Customer, DomainError> {
        if req
            .display_name
            .as_ref()
            .is_some_and(|n| n.trim().is_empty())
        {
            return Err(DomainError::Validation {
                details: HashMap::from([(
                    "display_name".into(),
                    vec!["Display name is required".into()],
                )]),
            });
        }
        let mut normalized = req.clone();
        if let Some(ref name) = normalized.display_name {
            normalized.display_name = Some(name.trim().to_string());
        }
        self.repo.update(id, &normalized).await
    }

    pub async fn soft_delete(&self, id: &CustomerId) -> Result<Customer, DomainError> {
        self.repo.soft_delete(id).await
    }
}
