//! SeaORM entity for the `kikan_meta` key/value table.
//!
//! The table is defined by [`crate::migrations::bootstrap`] (CreateKikanMeta)
//! as `(key TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID`. It stores
//! infrastructure-level metadata such as LAN access consent.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "kikan_meta")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub key: String,
    pub value: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
