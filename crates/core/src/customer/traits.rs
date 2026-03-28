use crate::actor::Actor;
use crate::customer::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use crate::error::DomainError;
use crate::filter::IncludeDeleted;
use crate::pagination::PageParams;

/// Port for customer persistence operations.
pub trait CustomerRepository: Send + Sync {
    fn find_by_id(
        &self,
        id: &CustomerId,
        filter: IncludeDeleted,
    ) -> impl Future<Output = Result<Option<Customer>, DomainError>> + Send;

    fn list(
        &self,
        params: PageParams,
        filter: IncludeDeleted,
        search: Option<&str>,
    ) -> impl Future<Output = Result<(Vec<Customer>, i64), DomainError>> + Send;

    fn create(
        &self,
        req: &CreateCustomer,
        actor: &Actor,
    ) -> impl Future<Output = Result<Customer, DomainError>> + Send;

    fn update(
        &self,
        id: &CustomerId,
        req: &UpdateCustomer,
        actor: &Actor,
    ) -> impl Future<Output = Result<Customer, DomainError>> + Send;

    fn soft_delete(
        &self,
        id: &CustomerId,
        actor: &Actor,
    ) -> impl Future<Output = Result<Customer, DomainError>> + Send;

    fn restore(
        &self,
        id: &CustomerId,
        actor: &Actor,
    ) -> impl Future<Output = Result<Customer, DomainError>> + Send;
}
