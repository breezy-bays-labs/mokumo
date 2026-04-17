use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::customer::{CreateCustomer, Customer, CustomerId, CustomerRepository, UpdateCustomer};
use mokumo_core::actor::Actor;
use mokumo_core::error::DomainError;
use mokumo_core::filter::IncludeDeleted;
use mokumo_core::pagination::PageParams;

// PARITY: must match phone regex in apps/web/src/lib/schemas/customer.ts
static PHONE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+]?[\d\s\-().]+$").unwrap());

pub struct CustomerService<R> {
    repo: R,
}

fn validate_phone(phone: &str) -> Result<(), String> {
    if !PHONE_RE.is_match(phone) || !phone.chars().any(|c| c.is_ascii_digit()) {
        return Err("Invalid phone number format".into());
    }
    Ok(())
}

// PARITY: must match address check in apps/web/src/lib/schemas/customer.ts
fn validate_address(value: &str) -> Result<(), String> {
    if !value.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err("Address contains invalid characters".into());
    }
    Ok(())
}

fn validate_contact_fields(
    phone: Option<&str>,
    address_line1: Option<&str>,
    address_line2: Option<&str>,
) -> Result<(), DomainError> {
    let mut details: HashMap<String, Vec<String>> = HashMap::new();

    let mut check =
        |field: &str, value: Option<&str>, validator: fn(&str) -> Result<(), String>| {
            if let Some(v) = value
                && !v.is_empty()
                && let Err(msg) = validator(v)
            {
                details.entry(field.into()).or_default().push(msg);
            }
        };

    check("phone", phone, validate_phone);
    check("address_line1", address_line1, validate_address);
    check("address_line2", address_line2, validate_address);

    if details.is_empty() {
        Ok(())
    } else {
        Err(DomainError::Validation { details })
    }
}

impl<R: CustomerRepository> CustomerService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(
        &self,
        id: &CustomerId,
        filter: IncludeDeleted,
    ) -> Result<Option<Customer>, DomainError> {
        self.repo.find_by_id(id, filter).await
    }

    pub async fn list(
        &self,
        params: PageParams,
        filter: IncludeDeleted,
        search: Option<&str>,
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        self.repo.list(params, filter, search).await
    }

    pub async fn create(
        &self,
        req: &CreateCustomer,
        actor: &Actor,
    ) -> Result<Customer, DomainError> {
        if req.display_name.trim().is_empty() {
            return Err(DomainError::Validation {
                details: HashMap::from([(
                    "display_name".into(),
                    vec!["Display name is required".into()],
                )]),
            });
        }
        validate_contact_fields(
            req.phone.as_deref(),
            req.address_line1.as_deref(),
            req.address_line2.as_deref(),
        )?;
        let mut normalized = req.clone();
        normalized.display_name = req.display_name.trim().to_string();
        self.repo.create(&normalized, actor).await
    }

    pub async fn update(
        &self,
        id: &CustomerId,
        req: &UpdateCustomer,
        actor: &Actor,
    ) -> Result<Customer, DomainError> {
        if req
            .display_name
            .as_ref()
            .is_some_and(|n| n.trim().is_empty())
        {
            return Err(DomainError::Validation {
                details: HashMap::from([(
                    "display_name".into(),
                    vec!["Display name is required".into()],
                )]),
            });
        }
        // For UpdateCustomer, phone/address use Option<Option<String>> (double option).
        // Some(Some(val)) = update to val, Some(None) = clear, None = don't touch.
        // We only validate when setting a new value: Some(Some(val)).
        let phone = req.phone.as_ref().and_then(|o| o.as_deref());
        let addr1 = req.address_line1.as_ref().and_then(|o| o.as_deref());
        let addr2 = req.address_line2.as_ref().and_then(|o| o.as_deref());
        validate_contact_fields(phone, addr1, addr2)?;
        let mut normalized = req.clone();
        if let Some(ref name) = normalized.display_name {
            normalized.display_name = Some(name.trim().to_string());
        }
        self.repo.update(id, &normalized, actor).await
    }

    pub async fn soft_delete(
        &self,
        id: &CustomerId,
        actor: &Actor,
    ) -> Result<Customer, DomainError> {
        self.repo.soft_delete(id, actor).await
    }

    pub async fn restore(&self, id: &CustomerId, actor: &Actor) -> Result<Customer, DomainError> {
        self.repo.restore(id, actor).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Unit tests for validate_phone ---

    #[test]
    fn phone_accepts_digits_only() {
        assert!(validate_phone("5551234567").is_ok());
    }

    #[test]
    fn phone_accepts_us_format() {
        assert!(validate_phone("(555) 123-4567").is_ok());
    }

    #[test]
    fn phone_accepts_international() {
        assert!(validate_phone("+1 555 123 4567").is_ok());
    }

    #[test]
    fn phone_accepts_dots() {
        assert!(validate_phone("555.123.4567").is_ok());
    }

    #[test]
    fn phone_rejects_letters() {
        assert!(validate_phone("555-CALL-ME").is_err());
    }

    #[test]
    fn phone_rejects_special_chars() {
        assert!(validate_phone("!@#$%^&*").is_err());
    }

    #[test]
    fn phone_rejects_only_dashes() {
        assert!(validate_phone("---").is_err());
    }

    // --- Unit tests for validate_address ---

    #[test]
    fn address_accepts_typical_street() {
        assert!(validate_address("123 Main St").is_ok());
    }

    #[test]
    fn address_accepts_unit_number() {
        assert!(validate_address("456 Oak Ave #200").is_ok());
    }

    #[test]
    fn address_rejects_pure_special_chars() {
        assert!(validate_address("!@#$%^&*").is_err());
    }

    #[test]
    fn address_accepts_mixed_chars() {
        assert!(validate_address("#200-A").is_ok());
    }

    // --- Integration tests for validate_contact_fields ---

    fn assert_validation_fields(err: DomainError, expected_fields: &[&str]) {
        if let DomainError::Validation { details } = err {
            for field in expected_fields {
                assert!(
                    details.contains_key(*field),
                    "Expected validation error for '{field}', got keys: {:?}",
                    details.keys().collect::<Vec<_>>()
                );
            }
        } else {
            panic!("Expected Validation error, got: {err:?}");
        }
    }

    #[test]
    fn contact_fields_accepts_all_none() {
        assert!(validate_contact_fields(None, None, None).is_ok());
    }

    #[test]
    fn contact_fields_accepts_empty_strings() {
        assert!(validate_contact_fields(Some(""), Some(""), Some("")).is_ok());
    }

    #[test]
    fn contact_fields_rejects_bad_phone() {
        let err = validate_contact_fields(Some("abc"), None, None).unwrap_err();
        assert_validation_fields(err, &["phone"]);
    }

    #[test]
    fn contact_fields_rejects_bad_address() {
        let err = validate_contact_fields(None, Some("!!!"), None).unwrap_err();
        assert_validation_fields(err, &["address_line1"]);
    }

    #[test]
    fn contact_fields_collects_multiple_errors() {
        let err = validate_contact_fields(Some("abc"), Some("!!!"), Some("@@@")).unwrap_err();
        assert_validation_fields(err, &["phone", "address_line1", "address_line2"]);
    }

    // --- Service-level tests with mock repo ---

    struct MockRepo;

    fn default_customer() -> Customer {
        Customer {
            id: CustomerId::generate(),
            display_name: "Test".into(),
            company_name: None,
            email: None,
            phone: None,
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
            notes: None,
            portal_enabled: false,
            portal_user_id: None,
            tax_exempt: false,
            tax_exemption_certificate_path: None,
            tax_exemption_expires_at: None,
            payment_terms: None,
            credit_limit_cents: None,
            stripe_customer_id: None,
            quickbooks_customer_id: None,
            lead_source: None,
            tags: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        }
    }

    impl CustomerRepository for MockRepo {
        async fn find_by_id(
            &self,
            _id: &CustomerId,
            _filter: IncludeDeleted,
        ) -> Result<Option<Customer>, DomainError> {
            Ok(None)
        }

        async fn list(
            &self,
            _params: PageParams,
            _filter: IncludeDeleted,
            _search: Option<&str>,
        ) -> Result<(Vec<Customer>, i64), DomainError> {
            Ok((vec![], 0))
        }

        async fn create(
            &self,
            req: &CreateCustomer,
            _actor: &Actor,
        ) -> Result<Customer, DomainError> {
            Ok(Customer {
                display_name: req.display_name.clone(),
                ..default_customer()
            })
        }

        async fn update(
            &self,
            _id: &CustomerId,
            _req: &UpdateCustomer,
            _actor: &Actor,
        ) -> Result<Customer, DomainError> {
            Ok(Customer {
                display_name: "Updated".into(),
                ..default_customer()
            })
        }

        async fn soft_delete(
            &self,
            _id: &CustomerId,
            _actor: &Actor,
        ) -> Result<Customer, DomainError> {
            unimplemented!()
        }

        async fn restore(&self, _id: &CustomerId, _actor: &Actor) -> Result<Customer, DomainError> {
            unimplemented!()
        }
    }

    fn svc() -> CustomerService<MockRepo> {
        CustomerService::new(MockRepo)
    }

    fn actor() -> Actor {
        Actor::system()
    }

    fn default_create() -> CreateCustomer {
        CreateCustomer {
            display_name: String::new(),
            company_name: None,
            email: None,
            phone: None,
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
            notes: None,
            portal_enabled: None,
            tax_exempt: None,
            payment_terms: None,
            credit_limit_cents: None,
            lead_source: None,
            tags: None,
        }
    }

    #[tokio::test]
    async fn create_rejects_invalid_phone() {
        let req = CreateCustomer {
            display_name: "Test".into(),
            phone: Some("abc-xyz".into()),
            ..default_create()
        };
        let err = svc().create(&req, &actor()).await.unwrap_err();
        assert_validation_fields(err, &["phone"]);
    }

    #[tokio::test]
    async fn create_accepts_valid_phone() {
        let req = CreateCustomer {
            display_name: "Test".into(),
            phone: Some("(555) 123-4567".into()),
            ..default_create()
        };
        assert!(svc().create(&req, &actor()).await.is_ok());
    }

    #[tokio::test]
    async fn create_rejects_garbage_address() {
        let req = CreateCustomer {
            display_name: "Test".into(),
            address_line1: Some("!@#$%".into()),
            ..default_create()
        };
        let err = svc().create(&req, &actor()).await.unwrap_err();
        assert_validation_fields(err, &["address_line1"]);
    }

    #[tokio::test]
    async fn create_accepts_valid_address() {
        let req = CreateCustomer {
            display_name: "Test".into(),
            address_line1: Some("123 Main St".into()),
            address_line2: Some("Suite 200".into()),
            ..default_create()
        };
        assert!(svc().create(&req, &actor()).await.is_ok());
    }

    #[tokio::test]
    async fn create_collects_phone_and_address_errors() {
        let req = CreateCustomer {
            display_name: "Test".into(),
            phone: Some("abc".into()),
            address_line1: Some("!!!".into()),
            ..default_create()
        };
        let err = svc().create(&req, &actor()).await.unwrap_err();
        assert_validation_fields(err, &["phone", "address_line1"]);
    }

    #[tokio::test]
    async fn update_rejects_invalid_phone() {
        let id = CustomerId::generate();
        let req = UpdateCustomer {
            phone: Some(Some("letters!".into())),
            ..Default::default()
        };
        let err = svc().update(&id, &req, &actor()).await.unwrap_err();
        assert_validation_fields(err, &["phone"]);
    }

    #[tokio::test]
    async fn update_accepts_clearing_phone() {
        let id = CustomerId::generate();
        let req = UpdateCustomer {
            phone: Some(None),
            ..Default::default()
        };
        assert!(svc().update(&id, &req, &actor()).await.is_ok());
    }

    #[tokio::test]
    async fn update_accepts_clearing_address() {
        let id = CustomerId::generate();
        let req = UpdateCustomer {
            address_line1: Some(None),
            address_line2: Some(None),
            ..Default::default()
        };
        assert!(svc().update(&id, &req, &actor()).await.is_ok());
    }

    #[tokio::test]
    async fn update_skips_validation_when_fields_untouched() {
        let id = CustomerId::generate();
        let req = UpdateCustomer {
            display_name: Some("New Name".into()),
            ..Default::default()
        };
        assert!(svc().update(&id, &req, &actor()).await.is_ok());
    }

    #[tokio::test]
    async fn update_rejects_garbage_address() {
        let id = CustomerId::generate();
        let req = UpdateCustomer {
            address_line1: Some(Some("@@@".into())),
            ..Default::default()
        };
        let err = svc().update(&id, &req, &actor()).await.unwrap_err();
        assert_validation_fields(err, &["address_line1"]);
    }
}
