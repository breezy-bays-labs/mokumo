//! API DTO types for the Mokumo shop vertical.
//!
//! Wire shapes consumed by the SvelteKit frontend. ts-rs bindings are emitted
//! into `apps/web/src/lib/types/shop/` via the `api:gen-types-shop` Moon task.

pub mod customer;

pub use customer::CustomerResponse;
