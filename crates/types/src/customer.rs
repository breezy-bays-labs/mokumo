use serde::Serialize;
use ts_rs::TS;

/// API response DTO for a customer record.
///
/// The `id` field is a String (UUID as text for JSON). Mapping from
/// `core::Customer` happens in the API layer via `From` impl.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CustomerResponse {
    pub id: String,
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
    pub tax_exemption_expires_at: Option<String>,
    pub payment_terms: Option<String>,
    #[ts(type = "number | null")]
    pub credit_limit_cents: Option<i64>,
    pub stripe_customer_id: Option<String>,
    pub quickbooks_customer_id: Option<String>,
    pub lead_source: Option<String>,
    pub tags: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}
