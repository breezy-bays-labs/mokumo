pub mod guards;
mod profile_dir_name;
mod profile_id;
pub mod resolve;

pub use profile_dir_name::ProfileDirName;
pub use profile_id::ProfileId;

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
