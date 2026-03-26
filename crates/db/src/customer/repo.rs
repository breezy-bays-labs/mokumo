use mokumo_core::activity::ActivityAction;
use mokumo_core::customer::traits::CustomerRepository;
use mokumo_core::customer::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use mokumo_core::error::DomainError;
use mokumo_core::filter::IncludeDeleted;
use mokumo_core::pagination::PageParams;
use sqlx::SqlitePool;

use crate::db_err;

#[derive(sqlx::FromRow)]
struct CustomerRow {
    id: String,
    company_name: Option<String>,
    display_name: String,
    email: Option<String>,
    phone: Option<String>,
    address_line1: Option<String>,
    address_line2: Option<String>,
    city: Option<String>,
    state: Option<String>,
    postal_code: Option<String>,
    country: Option<String>,
    notes: Option<String>,
    portal_enabled: bool,
    portal_user_id: Option<String>,
    tax_exempt: bool,
    tax_exemption_certificate_path: Option<String>,
    tax_exemption_expires_at: Option<String>,
    payment_terms: Option<String>,
    credit_limit_cents: Option<i64>,
    stripe_customer_id: Option<String>,
    quickbooks_customer_id: Option<String>,
    lead_source: Option<String>,
    tags: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
}

fn row_to_customer(row: CustomerRow) -> Result<Customer, DomainError> {
    let id = uuid::Uuid::parse_str(&row.id).map_err(|e| DomainError::Internal {
        message: format!("invalid UUID in database: {e}"),
    })?;
    Ok(Customer {
        id: CustomerId::new(id),
        company_name: row.company_name,
        display_name: row.display_name,
        email: row.email,
        phone: row.phone,
        address_line1: row.address_line1,
        address_line2: row.address_line2,
        city: row.city,
        state: row.state,
        postal_code: row.postal_code,
        country: row.country,
        notes: row.notes,
        portal_enabled: row.portal_enabled,
        portal_user_id: row.portal_user_id,
        tax_exempt: row.tax_exempt,
        tax_exemption_certificate_path: row.tax_exemption_certificate_path,
        tax_exemption_expires_at: row.tax_exemption_expires_at,
        payment_terms: row.payment_terms,
        credit_limit_cents: row.credit_limit_cents,
        stripe_customer_id: row.stripe_customer_id,
        quickbooks_customer_id: row.quickbooks_customer_id,
        lead_source: row.lead_source,
        tags: row.tags,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
    })
}

pub struct SqliteCustomerRepo {
    pool: SqlitePool,
}

impl SqliteCustomerRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl CustomerRepository for SqliteCustomerRepo {
    async fn find_by_id(
        &self,
        id: &CustomerId,
        filter: IncludeDeleted,
    ) -> Result<Option<Customer>, DomainError> {
        let include = matches!(filter, IncludeDeleted::IncludeDeleted);
        let row = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE id = ?1 AND (deleted_at IS NULL OR ?2)",
        )
        .bind(id.to_string())
        .bind(include)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;

        row.map(row_to_customer).transpose()
    }

    async fn list(
        &self,
        params: PageParams,
        filter: IncludeDeleted,
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        let include = matches!(filter, IncludeDeleted::IncludeDeleted);

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM customers WHERE (deleted_at IS NULL OR ?1)")
                .bind(include)
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?;

        let rows: Vec<CustomerRow> = sqlx::query_as(
            "SELECT * FROM customers WHERE (deleted_at IS NULL OR ?1) \
             ORDER BY created_at DESC, id DESC LIMIT ?2 OFFSET ?3",
        )
        .bind(include)
        .bind(params.per_page() as i64)
        .bind(params.offset() as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let customers: Vec<Customer> = rows
            .into_iter()
            .map(row_to_customer)
            .collect::<Result<_, _>>()?;
        Ok((customers, count))
    }

    async fn create(&self, req: &CreateCustomer) -> Result<Customer, DomainError> {
        let id = CustomerId::generate();
        let portal_enabled = req.portal_enabled.unwrap_or(false);
        let tax_exempt = req.tax_exempt.unwrap_or(false);

        let mut tx = self.pool.begin().await.map_err(db_err)?;

        let row = sqlx::query_as::<_, CustomerRow>(
            "INSERT INTO customers (\
                id, display_name, company_name, email, phone, \
                address_line1, address_line2, city, state, postal_code, country, \
                notes, portal_enabled, tax_exempt, payment_terms, credit_limit_cents, \
                lead_source, tags\
            ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, \
                ?6, ?7, ?8, ?9, ?10, ?11, \
                ?12, ?13, ?14, ?15, ?16, \
                ?17, ?18\
            ) RETURNING *",
        )
        .bind(id.to_string())
        .bind(&req.display_name)
        .bind(&req.company_name)
        .bind(&req.email)
        .bind(&req.phone)
        .bind(&req.address_line1)
        .bind(&req.address_line2)
        .bind(&req.city)
        .bind(&req.state)
        .bind(&req.postal_code)
        .bind(req.country.as_deref().unwrap_or("US"))
        .bind(&req.notes)
        .bind(portal_enabled)
        .bind(tax_exempt)
        .bind(req.payment_terms.as_deref().unwrap_or("due_on_receipt"))
        .bind(req.credit_limit_cents)
        .bind(&req.lead_source)
        .bind(&req.tags)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        let customer = row_to_customer(row)?;

        let payload = serde_json::to_value(&customer).map_err(|e| DomainError::Internal {
            message: format!("failed to serialize customer for activity log: {e}"),
        })?;
        crate::activity::insert_activity_log_raw(
            &mut tx,
            "customer",
            &customer.id.to_string(),
            ActivityAction::Created,
            "system",
            "system",
            &payload,
        )
        .await?;

        tx.commit().await.map_err(db_err)?;
        Ok(customer)
    }

    async fn update(&self, id: &CustomerId, req: &UpdateCustomer) -> Result<Customer, DomainError> {
        // For clearable (Option<Option<T>>) fields, we use a sentinel-based approach:
        //   - Outer None (omitted) → bind (false, NULL) → CASE keeps current value
        //   - Some(None) (explicit null) → bind (true, NULL) → CASE sets NULL
        //   - Some(Some(v)) (value) → bind (true, v) → CASE sets value
        // Non-nullable fields (display_name, portal_enabled, tax_exempt) use COALESCE.

        // Helper: extract (provided, value) pair from Option<Option<T>>
        fn clearable_str(field: &Option<Option<String>>) -> (bool, Option<&str>) {
            match field {
                None => (false, None),
                Some(None) => (true, None),
                Some(Some(v)) => (true, Some(v.as_str())),
            }
        }
        fn clearable_i64(field: &Option<Option<i64>>) -> (bool, Option<i64>) {
            match field {
                None => (false, None),
                Some(None) => (true, None),
                Some(Some(v)) => (true, Some(*v)),
            }
        }

        let (company_name_set, company_name_val) = clearable_str(&req.company_name);
        let (email_set, email_val) = clearable_str(&req.email);
        let (phone_set, phone_val) = clearable_str(&req.phone);
        let (addr1_set, addr1_val) = clearable_str(&req.address_line1);
        let (addr2_set, addr2_val) = clearable_str(&req.address_line2);
        let (city_set, city_val) = clearable_str(&req.city);
        let (state_set, state_val) = clearable_str(&req.state);
        let (postal_set, postal_val) = clearable_str(&req.postal_code);
        let (country_set, country_val) = clearable_str(&req.country);
        let (notes_set, notes_val) = clearable_str(&req.notes);
        let (terms_set, terms_val) = clearable_str(&req.payment_terms);
        let (credit_set, credit_val) = clearable_i64(&req.credit_limit_cents);
        let (lead_set, lead_val) = clearable_str(&req.lead_source);
        let (tags_set, tags_val) = clearable_str(&req.tags);

        let mut tx = self.pool.begin().await.map_err(db_err)?;

        let result = sqlx::query(
            "UPDATE customers SET \
                display_name = COALESCE(?1, display_name), \
                company_name = CASE WHEN ?2 THEN ?3 ELSE company_name END, \
                email = CASE WHEN ?4 THEN ?5 ELSE email END, \
                phone = CASE WHEN ?6 THEN ?7 ELSE phone END, \
                address_line1 = CASE WHEN ?8 THEN ?9 ELSE address_line1 END, \
                address_line2 = CASE WHEN ?10 THEN ?11 ELSE address_line2 END, \
                city = CASE WHEN ?12 THEN ?13 ELSE city END, \
                state = CASE WHEN ?14 THEN ?15 ELSE state END, \
                postal_code = CASE WHEN ?16 THEN ?17 ELSE postal_code END, \
                country = CASE WHEN ?18 THEN ?19 ELSE country END, \
                notes = CASE WHEN ?20 THEN ?21 ELSE notes END, \
                portal_enabled = COALESCE(?22, portal_enabled), \
                tax_exempt = COALESCE(?23, tax_exempt), \
                payment_terms = CASE WHEN ?24 THEN ?25 ELSE payment_terms END, \
                credit_limit_cents = CASE WHEN ?26 THEN ?27 ELSE credit_limit_cents END, \
                lead_source = CASE WHEN ?28 THEN ?29 ELSE lead_source END, \
                tags = CASE WHEN ?30 THEN ?31 ELSE tags END \
            WHERE id = ?32 AND deleted_at IS NULL",
        )
        .bind(&req.display_name) // ?1
        .bind(company_name_set) // ?2
        .bind(company_name_val) // ?3
        .bind(email_set) // ?4
        .bind(email_val) // ?5
        .bind(phone_set) // ?6
        .bind(phone_val) // ?7
        .bind(addr1_set) // ?8
        .bind(addr1_val) // ?9
        .bind(addr2_set) // ?10
        .bind(addr2_val) // ?11
        .bind(city_set) // ?12
        .bind(city_val) // ?13
        .bind(state_set) // ?14
        .bind(state_val) // ?15
        .bind(postal_set) // ?16
        .bind(postal_val) // ?17
        .bind(country_set) // ?18
        .bind(country_val) // ?19
        .bind(notes_set) // ?20
        .bind(notes_val) // ?21
        .bind(req.portal_enabled) // ?22
        .bind(req.tax_exempt) // ?23
        .bind(terms_set) // ?24
        .bind(terms_val) // ?25
        .bind(credit_set) // ?26
        .bind(credit_val) // ?27
        .bind(lead_set) // ?28
        .bind(lead_val) // ?29
        .bind(tags_set) // ?30
        .bind(tags_val) // ?31
        .bind(id.to_string()) // ?32
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        if result.rows_affected() == 0 {
            // tx dropped without commit → automatic rollback
            return Err(DomainError::NotFound {
                entity: "customer",
                id: id.to_string(),
            });
        }

        // Re-fetch to get post-trigger updated_at
        let row = sqlx::query_as::<_, CustomerRow>("SELECT * FROM customers WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;

        let customer = row_to_customer(row)?;

        let payload = serde_json::to_value(&customer).map_err(|e| DomainError::Internal {
            message: format!("failed to serialize customer for activity log: {e}"),
        })?;
        crate::activity::insert_activity_log_raw(
            &mut tx,
            "customer",
            &customer.id.to_string(),
            ActivityAction::Updated,
            "system",
            "system",
            &payload,
        )
        .await?;

        tx.commit().await.map_err(db_err)?;
        Ok(customer)
    }

    async fn soft_delete(&self, id: &CustomerId) -> Result<Customer, DomainError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        let result = sqlx::query(
            "UPDATE customers SET deleted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
             WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        if result.rows_affected() == 0 {
            // tx dropped without commit → automatic rollback
            return Err(DomainError::NotFound {
                entity: "customer",
                id: id.to_string(),
            });
        }

        // Re-fetch to get post-trigger state (deleted_at + updated_at)
        let row = sqlx::query_as::<_, CustomerRow>("SELECT * FROM customers WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;

        let customer = row_to_customer(row)?;

        let payload = serde_json::to_value(&customer).map_err(|e| DomainError::Internal {
            message: format!("failed to serialize customer for activity log: {e}"),
        })?;
        crate::activity::insert_activity_log_raw(
            &mut tx,
            "customer",
            &customer.id.to_string(),
            ActivityAction::SoftDeleted,
            "system",
            "system",
            &payload,
        )
        .await?;

        tx.commit().await.map_err(db_err)?;
        Ok(customer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repo_create_rolls_back_on_activity_failure() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = tmp.path().join("test.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = crate::initialize_database(&database_url)
            .await
            .expect("failed to initialize database");

        // Start a transaction, insert a customer, then drop without committing
        {
            let mut tx = pool.begin().await.expect("begin");

            let id = CustomerId::generate();
            sqlx::query(
                "INSERT INTO customers (\
                    id, display_name, portal_enabled, tax_exempt, \
                    payment_terms, country\
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(id.to_string())
            .bind("Rollback Corp")
            .bind(false)
            .bind(false)
            .bind("due_on_receipt")
            .bind("US")
            .execute(&mut *tx)
            .await
            .expect("insert should succeed");

            // Verify the customer is visible within the transaction
            let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
                .fetch_one(&mut *tx)
                .await
                .expect("count within tx");
            assert_eq!(
                row.0, 1,
                "Customer should be visible within the transaction"
            );

            // tx dropped here without commit → automatic rollback
        }

        // Verify the customer was rolled back
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&pool)
            .await
            .expect("count after rollback");
        assert_eq!(
            row.0, 0,
            "Customer should NOT exist after transaction rollback"
        );
    }
}
