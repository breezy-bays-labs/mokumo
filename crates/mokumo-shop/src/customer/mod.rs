//! Customer vertical slice — domain, entity, SeaORM adapter, service,
//! handler, and DTOs colocated.

pub mod adapter;
pub mod domain;
pub mod entity;
pub mod error;
pub mod handler;
pub mod service;

pub use adapter::SqliteCustomerRepository;
pub use domain::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
pub use error::CustomerHandlerError;
pub use handler::{CustomerRouterDeps, customer_router};
pub use service::CustomerService;

use kikan::actor::Actor;
use kikan::error::DomainError;
use kikan::filter::IncludeDeleted;
use kikan::pagination::PageParams;

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
