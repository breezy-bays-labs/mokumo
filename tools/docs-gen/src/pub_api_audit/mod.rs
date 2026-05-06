//! Public-API spec audit (mokumo#654).
//!
//! Enumerates every `pub` item across the workspace via syn (sibling
//! pattern to [`crate::coverage::route_walker`]) and joins it with
//! BDD-only line coverage (lcov from `cargo llvm-cov nextest -E
//! 'binary(=bdd) | binary(=api_bdd) | binary(=platform_bdd)'`) to
//! answer the question: "is this defined `pub` item exercised by ANY
//! cucumber scenario?"
//!
//! Non-goals:
//! - External-API surface (= what external consumers see). `pub use`
//!   re-exports don't matter for the BDD-coverage question; we anchor
//!   on definition site.
//! - Per-handler branch coverage (mokumo#583's lane).
//! - Per-route scenario taxonomy (mokumo#655's lane — already shipped).
//!
//! Attribution semantics: an item is "covered" iff at least one source
//! line in its `[span_begin..=span_end]` range registers ≥ 1 lcov hit.
//! Documented in [`docs/adr/adr-pub-api-spec-audit.md`]; permissive but
//! matches the question this gate asks ("does ANY scenario hit this?").

pub mod artifact;
pub mod cli;
pub mod lcov_loader;
pub mod markdown;
pub mod producer;
pub mod pub_walker;

pub use cli::execute;
pub use producer::{ProducerError, ProducerInput, ProducerOutput, run};
