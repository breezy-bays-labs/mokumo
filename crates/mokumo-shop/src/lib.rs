//! Mokumo shop vertical — **neutral shop core.**
//!
//! The types and services in this crate are intended to generalize
//! across shop-style businesses: customers, shop settings, sequences,
//! quotes, invoices, orders, kanban workflow, generic inventory
//! (passthrough/consumable), products, cost+markup pricing, and shop
//! financials.
//!
//! Decorator-specific concepts — garments as substrates, artwork
//! pipelines, method-specific pricing (screenprint tiers, embroidery
//! stitch counts), mockup generators — do not belong here. They are
//! intended for a separate `mokumo-decor` crate layered on top, and
//! individual method crates layered on top of that. Growth is
//! additive: new crates sit above the neutral core; the neutral core
//! is never re-extracted from a specialized crate.

pub mod activity;
pub mod customer;
pub mod types;

pub use activity::ActivityAction;
pub use customer::{
    CreateCustomer, Customer, CustomerHandlerError, CustomerId, CustomerRepository,
    CustomerRouterDeps, CustomerService, SqliteCustomerRepository, UpdateCustomer, customer_router,
};
pub use types::CustomerResponse;
