use mokumo_core::activity::ActivityAction;
use mokumo_core::customer::traits::CustomerRepository;
use mokumo_core::customer::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use mokumo_core::error::DomainError;
use mokumo_core::filter::IncludeDeleted;
use mokumo_core::pagination::PageParams;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    IntoActiveValue, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};

use super::entity::{self, Entity as CustomerEntity};
use crate::sea_err;

impl From<entity::Model> for Customer {
    fn from(m: entity::Model) -> Self {
        Customer {
            id: CustomerId::new(m.id),
            company_name: m.company_name,
            display_name: m.display_name,
            email: m.email,
            phone: m.phone,
            address_line1: m.address_line1,
            address_line2: m.address_line2,
            city: m.city,
            state: m.state,
            postal_code: m.postal_code,
            country: m.country,
            notes: m.notes,
            portal_enabled: m.portal_enabled,
            portal_user_id: m.portal_user_id,
            tax_exempt: m.tax_exempt,
            tax_exemption_certificate_path: m.tax_exemption_certificate_path,
            tax_exemption_expires_at: m.tax_exemption_expires_at,
            payment_terms: m.payment_terms,
            credit_limit_cents: m.credit_limit_cents,
            stripe_customer_id: m.stripe_customer_id,
            quickbooks_customer_id: m.quickbooks_customer_id,
            lead_source: m.lead_source,
            tags: m.tags,
            created_at: m.created_at,
            updated_at: m.updated_at,
            deleted_at: m.deleted_at,
        }
    }
}

/// Serialize the customer snapshot and insert an activity log entry within
/// the caller's transaction. Hardcodes actor to "system" until auth lands.
async fn log_customer_activity(
    conn: &impl ConnectionTrait,
    customer: &Customer,
    action: ActivityAction,
) -> Result<(), DomainError> {
    let payload = serde_json::to_value(customer).map_err(|e| DomainError::Internal {
        message: format!("failed to serialize customer for activity log: {e}"),
    })?;
    crate::activity::insert_activity_log_raw(
        conn,
        "customer",
        &customer.id.to_string(),
        action,
        "system",
        "system",
        &payload,
    )
    .await
}

pub struct SeaOrmCustomerRepo {
    db: DatabaseConnection,
}

impl SeaOrmCustomerRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl CustomerRepository for SeaOrmCustomerRepo {
    async fn find_by_id(
        &self,
        id: &CustomerId,
        filter: IncludeDeleted,
    ) -> Result<Option<Customer>, DomainError> {
        let mut query = CustomerEntity::find().filter(entity::Column::Id.eq(id.get()));

        if !matches!(filter, IncludeDeleted::IncludeDeleted) {
            query = query.filter(entity::Column::DeletedAt.is_null());
        }

        let model = query.one(&self.db).await.map_err(sea_err)?;
        Ok(model.map(Customer::from))
    }

    async fn list(
        &self,
        params: PageParams,
        filter: IncludeDeleted,
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        let include = matches!(filter, IncludeDeleted::IncludeDeleted);
        let mut base = CustomerEntity::find();
        if !include {
            base = base.filter(entity::Column::DeletedAt.is_null());
        }

        let count = base.clone().count(&self.db).await.map_err(sea_err)? as i64;

        let models = base
            .order_by_desc(entity::Column::CreatedAt)
            .order_by_desc(entity::Column::Id)
            .limit(Some(params.per_page() as u64))
            .offset(Some(params.offset() as u64))
            .all(&self.db)
            .await
            .map_err(sea_err)?;

        let customers = models.into_iter().map(Customer::from).collect();
        Ok((customers, count))
    }

    async fn create(&self, req: &CreateCustomer) -> Result<Customer, DomainError> {
        let id = CustomerId::generate();
        let txn = self.db.begin().await.map_err(sea_err)?;

        let active = entity::ActiveModel {
            id: ActiveValue::Set(id.get()),
            display_name: ActiveValue::Set(req.display_name.clone()),
            company_name: ActiveValue::Set(req.company_name.clone()),
            email: ActiveValue::Set(req.email.clone()),
            phone: ActiveValue::Set(req.phone.clone()),
            address_line1: ActiveValue::Set(req.address_line1.clone()),
            address_line2: ActiveValue::Set(req.address_line2.clone()),
            city: ActiveValue::Set(req.city.clone()),
            state: ActiveValue::Set(req.state.clone()),
            postal_code: ActiveValue::Set(req.postal_code.clone()),
            country: ActiveValue::Set(Some(
                req.country.clone().unwrap_or_else(|| "US".to_string()),
            )),
            notes: ActiveValue::Set(req.notes.clone()),
            portal_enabled: ActiveValue::Set(req.portal_enabled.unwrap_or(false)),
            portal_user_id: ActiveValue::NotSet,
            tax_exempt: ActiveValue::Set(req.tax_exempt.unwrap_or(false)),
            tax_exemption_certificate_path: ActiveValue::NotSet,
            tax_exemption_expires_at: ActiveValue::NotSet,
            payment_terms: ActiveValue::Set(Some(
                req.payment_terms
                    .clone()
                    .unwrap_or_else(|| "due_on_receipt".to_string()),
            )),
            credit_limit_cents: ActiveValue::Set(req.credit_limit_cents),
            stripe_customer_id: ActiveValue::NotSet,
            quickbooks_customer_id: ActiveValue::NotSet,
            lead_source: ActiveValue::Set(req.lead_source.clone()),
            tags: ActiveValue::Set(req.tags.clone()),
            created_at: ActiveValue::NotSet,
            updated_at: ActiveValue::NotSet,
            deleted_at: ActiveValue::NotSet,
        };

        let model = active.insert(&txn).await.map_err(sea_err)?;
        let customer = Customer::from(model);

        log_customer_activity(&txn, &customer, ActivityAction::Created).await?;

        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }

    async fn update(&self, id: &CustomerId, req: &UpdateCustomer) -> Result<Customer, DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;

        // Verify customer exists and is not soft-deleted
        let exists = CustomerEntity::find()
            .filter(entity::Column::Id.eq(id.get()))
            .filter(entity::Column::DeletedAt.is_null())
            .count(&txn)
            .await
            .map_err(sea_err)?;

        if exists == 0 {
            return Err(DomainError::NotFound {
                entity: "customer",
                id: id.to_string(),
            });
        }

        // Build ActiveModel with only changed fields
        let mut active = entity::ActiveModel {
            id: ActiveValue::Unchanged(id.get()),
            ..Default::default()
        };

        // Non-nullable fields: Option<T> -> Set if Some, NotSet if None
        if let Some(ref name) = req.display_name {
            active.display_name = ActiveValue::Set(name.clone());
        }
        if let Some(v) = req.portal_enabled {
            active.portal_enabled = ActiveValue::Set(v);
        }
        if let Some(v) = req.tax_exempt {
            active.tax_exempt = ActiveValue::Set(v);
        }

        // Clearable fields: Option<Option<T>> -> IntoActiveValue
        active.company_name = req.company_name.clone().into_active_value();
        active.email = req.email.clone().into_active_value();
        active.phone = req.phone.clone().into_active_value();
        active.address_line1 = req.address_line1.clone().into_active_value();
        active.address_line2 = req.address_line2.clone().into_active_value();
        active.city = req.city.clone().into_active_value();
        active.state = req.state.clone().into_active_value();
        active.postal_code = req.postal_code.clone().into_active_value();
        active.country = req.country.clone().into_active_value();
        active.notes = req.notes.clone().into_active_value();
        active.payment_terms = req.payment_terms.clone().into_active_value();
        active.credit_limit_cents = req.credit_limit_cents.into_active_value();
        active.lead_source = req.lead_source.clone().into_active_value();
        active.tags = req.tags.clone().into_active_value();

        active.update(&txn).await.map_err(sea_err)?;

        // Re-fetch for post-trigger updated_at
        let model = CustomerEntity::find_by_id(id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "customer disappeared mid-transaction".into(),
            })?;

        let customer = Customer::from(model);
        log_customer_activity(&txn, &customer, ActivityAction::Updated).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }

    async fn soft_delete(&self, id: &CustomerId) -> Result<Customer, DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;

        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let result = CustomerEntity::update_many()
            .col_expr(
                entity::Column::DeletedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(entity::Column::Id.eq(id.get()))
            .filter(entity::Column::DeletedAt.is_null())
            .exec(&txn)
            .await
            .map_err(sea_err)?;

        if result.rows_affected == 0 {
            return Err(DomainError::NotFound {
                entity: "customer",
                id: id.to_string(),
            });
        }

        // Re-fetch for post-trigger state (deleted_at + updated_at)
        let model = CustomerEntity::find_by_id(id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "customer disappeared mid-transaction".into(),
            })?;

        let customer = Customer::from(model);
        log_customer_activity(&txn, &customer, ActivityAction::SoftDeleted).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mokumo_core::customer::traits::CustomerRepository;

    async fn test_db() -> (
        DatabaseConnection,
        sqlx::sqlite::SqlitePool,
        tempfile::TempDir,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = crate::initialize_database(&url).await.unwrap();
        let pool = db.get_sqlite_connection_pool().clone();
        (db, pool, tmp)
    }

    /// Verifies SQLite transaction drop semantics: a transaction dropped
    /// without calling `commit()` automatically rolls back all changes.
    #[tokio::test]
    async fn sqlite_transaction_drop_rolls_back() {
        let (_db, pool, _tmp) = test_db().await;

        {
            let mut tx = pool.begin().await.unwrap();
            let id = CustomerId::generate();
            sqlx::query(
                "INSERT INTO customers (id, display_name, portal_enabled, tax_exempt, payment_terms, country) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(id.to_string())
            .bind("Rollback Corp")
            .bind(false)
            .bind(false)
            .bind("due_on_receipt")
            .bind("US")
            .execute(&mut *tx)
            .await
            .unwrap();
            // tx dropped here without commit
        }

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count.0, 0,
            "Customer should NOT exist after transaction rollback"
        );
    }

    /// Fault-injection test: dropping the `activity_log` table simulates
    /// an infrastructure failure during the activity logging step.
    /// The entire transaction must roll back — no orphaned customer rows.
    #[tokio::test]
    async fn create_rolls_back_when_activity_log_fails() {
        let (db, pool, _tmp) = test_db().await;

        sqlx::query("DROP TABLE activity_log")
            .execute(&pool)
            .await
            .unwrap();

        let repo = SeaOrmCustomerRepo::new(db);
        let req = CreateCustomer {
            display_name: "Fault Injection Corp".to_string(),
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
        };
        let result = repo.create(&req).await;
        assert!(
            result.is_err(),
            "create should fail when activity_log table is missing"
        );

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count.0, 0,
            "Customer row should NOT exist after activity log failure — transaction must roll back"
        );
    }
}
