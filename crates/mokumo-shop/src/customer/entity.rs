//! SeaORM entity for the `customers` table.
//!
//! Copied from `mokumo_db::customer::entity` during Stage 3. Schema unchanged.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "customers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub company_name: Option<String>,
    pub display_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub notes: Option<String>,
    pub portal_enabled: bool,
    pub portal_user_id: Option<String>,
    pub tax_exempt: bool,
    pub tax_exemption_certificate_path: Option<String>,
    pub tax_exemption_expires_at: Option<DateTimeUtc>,
    pub payment_terms: Option<String>,
    pub credit_limit_cents: Option<i64>,
    pub stripe_customer_id: Option<String>,
    pub quickbooks_customer_id: Option<String>,
    pub lead_source: Option<String>,
    pub tags: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub deleted_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
