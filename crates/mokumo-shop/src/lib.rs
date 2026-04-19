//! Mokumo shop Application — the decoration shop app grafted onto kikan.
//!
//! This crate is the `Application` in the kikan Engine vocabulary
//! (see `ops/decisions/mokumo/adr-kikan-engine-vocabulary.md`). It owns
//! the shop-domain surface: customers, shop settings, sequences, quotes,
//! invoices, orders, kanban workflow, generic inventory
//! (passthrough/consumable), products, cost+markup pricing, and shop
//! financials — plus the extension API surface that decoration
//! techniques plug into (see `ops/decisions/mokumo/adr-mokumo-extensions.md`).
//!
//! Decoration-technique-specific concepts — screen printing pricing
//! tiers, embroidery stitch-count math, DTF gang-sheet packing — belong
//! in `crates/extensions/mokumo-{screen-printing,embroidery,dtf,dtg}/`.
//! Those crates register with `mokumo-shop`'s `ExtensionRegistry` at
//! boot. Shared decoration primitives that accumulate across two or
//! more techniques may later be extracted into a `mokumo-decor` crate,
//! but that extraction is deferred until a concrete second-technique
//! consumer demands it. Until then, shared decoration primitives live
//! here.

pub mod activity;
pub mod cli;
pub mod customer;
pub mod db;
pub mod graft;
pub mod lifecycle;
pub mod migrations;
pub mod profile_db_init;
pub mod profile_switch;
pub mod restore;
pub mod restore_handler;
pub mod routes;
pub mod sequence;
pub mod server_info;
pub mod settings;
pub mod setup;
pub mod shop;
pub mod startup;
pub mod state;
pub mod types;
pub mod ws;

pub use activity::ActivityAction;
pub use customer::{
    CreateCustomer, Customer, CustomerHandlerError, CustomerId, CustomerRepository,
    CustomerRouterDeps, CustomerService, SqliteCustomerRepository, UpdateCustomer, customer_router,
};
pub use shop::{
    ShopLogoHandlerError, ShopLogoInfo, ShopLogoRepository, ShopLogoRouterDeps, ShopLogoService,
    SqliteShopLogoRepository, shop_logo_protected_router, shop_logo_public_router,
};
pub use types::CustomerResponse;
