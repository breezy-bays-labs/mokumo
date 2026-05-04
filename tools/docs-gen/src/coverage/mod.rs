//! Per-handler branch coverage producer (mokumo#583, scorecard V4
//! `Row::CoverageDelta.breakouts.by_crate[].handlers[]`).
//!
//! ```text
//! coverage-branches.json (cargo llvm-cov --branch on nightly)
//!         │
//!         ▼
//!   llvm_cov::parse  ──►  FunctionCoverage by demangled rust_path
//!         │
//!         │       routes/router.rs source files
//!         │              │
//!         │              ▼
//!         │       route_walker::walk  ──►  RouteEntry { method, path,
//!         │                                              handler_path,
//!         │                                              file, crate_name }
//!         │              │
//!         ▼              ▼
//!     producer::run     (join on rust_path)
//!         │
//!         ▼
//!   coverage-breakouts.json  (consumed by `scorecard aggregate
//!                             --coverage-breakouts-json`)
//! ```
//!
//! Failure modes — all loud:
//! - Route walker finds a handler symbol that the coverage payload doesn't
//!   list (function removed without route update, or vice-versa).
//! - Route walker can't resolve a bare handler ident against `use` items.
//! - Coverage payload is shaped differently than expected (LLVM schema
//!   drift; producer asserts presence of `data[0].functions[]`).
//!
//! The producer never falls back to "(no data)" — the aggregator's
//! `PENDING_TEXT_PREFIX` sentinel handles a missing artifact, but a present
//! artifact with malformed contents is a build failure, not silent partial
//! data.

pub mod artifact;
pub mod crap_exclusions;
pub mod llvm_cov;
pub mod producer;
pub mod route_walker;

pub use producer::{ProducerError, ProducerInput, ProducerOutput, run};
