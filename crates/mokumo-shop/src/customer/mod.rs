//! Customer vertical — domain, entity, adapter.
//!
//! Handler + DTO move here in V6c; deletion of the legacy
//! `crates/core/src/customer/`, `crates/db/src/customer/`, and
//! `services/api/src/customer/` happens in V6c.

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

use mokumo_core::actor::Actor;
use mokumo_core::error::DomainError;
use mokumo_core::filter::IncludeDeleted;
use mokumo_core::pagination::PageParams;

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
