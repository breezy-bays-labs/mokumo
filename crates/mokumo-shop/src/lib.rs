//! Mokumo shop vertical — **neutral shop core.**
//!
//! Passes the auto-repair-shop litmus test: customers, shop settings,
//! sequences, quotes, invoices, orders, kanban workflow, generic
//! inventory (passthrough/consumable), products, cost+markup pricing,
//! and shop financials all generalize across shop-style businesses.
//!
//! Decorator-specific concepts — garments as substrates, artwork
//! pipelines, method-specific pricing (screenprint tiers, embroidery
//! stitch counts), mockup generators — belong in a future
//! `mokumo-decor` crate layered on top of this core, and individual
//! method crates layered on top of that. Growth is additive: new crates
//! sit above the neutral core; the neutral core is never re-extracted
//! from a specialized crate.
//!
//! See `CLAUDE.md` → Crate stratification and the ADR at
//! `ops/decisions/mokumo/adr-neutral-core-additive-verticals.md`.

pub mod activity;
pub mod customer;
pub mod types;

pub use activity::ActivityAction;
pub use customer::{
    CreateCustomer, Customer, CustomerHandlerError, CustomerId, CustomerRepository,
    CustomerRouterDeps, CustomerService, SqliteCustomerRepository, UpdateCustomer, customer_router,
};
pub use types::CustomerResponse;
