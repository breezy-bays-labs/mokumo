//! Customer domain types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Strongly-typed customer identifier. Wraps a UUID v4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CustomerId(Uuid);

impl CustomerId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn get(self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for CustomerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for CustomerId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Domain entity representing a customer record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Customer {
    pub id: CustomerId,
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
    pub tax_exemption_expires_at: Option<DateTime<Utc>>,
    pub payment_terms: Option<String>,
    pub credit_limit_cents: Option<i64>,
    pub stripe_customer_id: Option<String>,
    pub quickbooks_customer_id: Option<String>,
    pub lead_source: Option<String>,
    pub tags: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Request to create a new customer. Only `display_name` is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCustomer {
    pub display_name: String,
    pub company_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub notes: Option<String>,
    pub portal_enabled: Option<bool>,
    pub tax_exempt: Option<bool>,
    pub payment_terms: Option<String>,
    pub credit_limit_cents: Option<i64>,
    pub lead_source: Option<String>,
    pub tags: Option<String>,
}

/// Request to update an existing customer. Nullable fields use
/// `Option<Option<T>>` with `serde_with::rust::double_option`:
/// - `None` → don't touch; `Some(None)` → clear; `Some(Some(v))` → set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateCustomer {
    pub display_name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub company_name: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub email: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub phone: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub address_line1: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub address_line2: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub city: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub state: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub postal_code: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub country: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub notes: Option<Option<String>>,
    pub portal_enabled: Option<bool>,
    pub tax_exempt: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub payment_terms: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub credit_limit_cents: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub lead_source: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub tags: Option<Option<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_id_display() {
        let id = CustomerId::generate();
        let display = id.to_string();
        assert!(!display.is_empty());
        let parsed: CustomerId = display.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn customer_id_serialize_roundtrip() {
        let id = CustomerId::generate();
        let json = serde_json::to_string(&id).unwrap();
        let restored: CustomerId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, restored);
    }

    #[test]
    fn customer_id_get_returns_inner() {
        let uuid = Uuid::new_v4();
        let id = CustomerId::new(uuid);
        assert_eq!(id.get(), uuid);
    }
}
