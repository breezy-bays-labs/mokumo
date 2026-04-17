//! Transitional re-export shim.
//!
//! Stage 3 (#507) lifted the activity-log reader + write helper into
//! `kikan::activity`. These re-exports keep `mokumo_db::activity::*` call
//! sites (services/api handlers, BDD worlds, entity repo adapters)
//! compiling while the crate is being dissolved. The shim disappears
//! alongside `crates/db` in S3.1b.

pub use kikan::activity::insert_activity_log_raw;

pub mod repo {
    pub use kikan::activity::SqliteActivityLogRepo;
}
