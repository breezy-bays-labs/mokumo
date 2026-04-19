//! API DTO types for the Mokumo shop vertical.
//!
//! Wire shapes consumed by the SvelteKit frontend. ts-rs bindings are emitted
//! into `apps/web/src/lib/types/shop/` via the `shop:gen-types-shop` Moon task.

pub mod customer;
pub mod error;

pub use customer::CustomerResponse;
pub use error::{ShopErrorBody, ShopErrorCode};
