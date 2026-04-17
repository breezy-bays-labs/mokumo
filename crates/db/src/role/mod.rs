//! Transitional re-export shim — role entity lives in `kikan::auth`.
//! Removed in S3.1b alongside `crates/db`.

pub mod entity {
    pub use kikan::auth::entity_role::{ActiveModel, Column, Entity, Model, PrimaryKey, Relation};
}
