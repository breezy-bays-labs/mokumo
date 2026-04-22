pub mod guards;
pub mod layout;
mod profile_dir_name;
mod profile_id;
pub mod resolve;

pub use profile_dir_name::ProfileDirName;
pub use profile_id::ProfileId;

// `SetupMode` remains in `kikan-types` (wire-contract DTO consumed by
// the SPA and `kikan-types` cannot depend on kikan without a cycle).
// Modules that need it import `kikan_types::SetupMode` directly; kikan
// no longer re-exports it — the type is no longer part of kikan's
// vocabulary (see `adr-kikan-engine-vocabulary`).

use std::path::{Path, PathBuf};

pub struct Tenancy {
    data_dir: PathBuf,
}

impl Tenancy {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

impl std::fmt::Debug for Tenancy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tenancy")
            .field("data_dir", &self.data_dir)
            .finish()
    }
}
