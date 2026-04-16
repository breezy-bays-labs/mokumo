//! Vertical-bound wrappers around [`kikan::backup::restore`].
//!
//! Pre-Stage-3 this module contained the full restore-candidate
//! validation + atomic copy implementation. Stage 3 (#507) lifted the
//! generic primitives into `kikan::backup::restore`; this module remains
//! as a thin wrapper that binds those primitives to the mokumo vertical's
//! [`crate::migration::Migrator`] and the `"mokumo.db"` production-slot
//! filename. It goes away with `crates/db` in S3.1b.

use std::path::Path;

pub use kikan::backup::{CandidateInfo, RestoreError};

/// Mokumo-vertical production-slot filename.
///
/// The `copy_to_production` primitive in `kikan::backup::restore` is
/// vertical-agnostic; this constant is the vertical's binding.
const MOKUMO_DB_FILENAME: &str = "mokumo.db";

/// Validate a `.db` file as a mokumo restore candidate.
///
/// Binds [`kikan::backup::restore::validate_candidate`] to
/// [`crate::migration::Migrator`] so callers don't need to name the
/// vertical's migrator type.
pub fn validate_candidate(source: &Path) -> Result<CandidateInfo, RestoreError> {
    kikan::backup::restore::validate_candidate::<crate::migration::Migrator>(source)
}

/// Copy a validated `.db` file to the mokumo production slot.
///
/// Binds [`kikan::backup::restore::copy_to_production`] to the mokumo
/// production-slot filename (`mokumo.db`).
pub fn copy_to_production(source: &Path, production_dir: &Path) -> Result<(), RestoreError> {
    kikan::backup::restore::copy_to_production(source, production_dir, MOKUMO_DB_FILENAME)
}
