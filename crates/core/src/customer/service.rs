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
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        self.repo.list(params, filter).await
    }

    pub async fn create(&self, req: &CreateCustomer) -> Result<Customer, DomainError> {
        self.repo.create(req).await
    }

    pub async fn update(
        &self,
        id: &CustomerId,
        req: &UpdateCustomer,
    ) -> Result<Customer, DomainError> {
        self.repo.update(id, req).await
    }

    pub async fn soft_delete(&self, id: &CustomerId) -> Result<Customer, DomainError> {
        self.repo.soft_delete(id).await
    }
}
