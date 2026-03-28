use mokumo_core::activity::ActivityAction;
use mokumo_core::actor::Actor;
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
/// the caller's transaction.
async fn log_customer_activity(
    conn: &impl ConnectionTrait,
    customer: &Customer,
    action: ActivityAction,
    actor: &Actor,
) -> Result<(), DomainError> {
    let payload = serde_json::to_value(customer).map_err(|e| DomainError::Internal {
        message: format!("failed to serialize customer for activity log: {e}"),
    })?;
    crate::activity::insert_activity_log_raw(
        conn,
        "customer",
        &customer.id.to_string(),
        action,
        actor.id(),
        &actor.actor_type().to_string(),
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
        search: Option<&str>,
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        let include = matches!(filter, IncludeDeleted::IncludeDeleted);
        let mut base = CustomerEntity::find();
        if !include {
            base = base.filter(entity::Column::DeletedAt.is_null());
        }

        // Search across display_name, company_name, and email with wildcard escaping
        if let Some(term) = search.filter(|s| !s.is_empty()) {
            let escaped = term
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            use sea_orm::Condition;
            use sea_orm::sea_query::LikeExpr;
            let like_expr = LikeExpr::new(&pattern).escape('\\');
            base = base.filter(
                Condition::any()
                    .add(entity::Column::DisplayName.like(like_expr.clone()))
                    .add(entity::Column::CompanyName.like(like_expr.clone()))
                    .add(entity::Column::Email.like(like_expr)),
            );
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

    async fn create(&self, req: &CreateCustomer, actor: &Actor) -> Result<Customer, DomainError> {
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

        log_customer_activity(&txn, &customer, ActivityAction::Created, actor).await?;

        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }

    async fn update(
        &self,
        id: &CustomerId,
        req: &UpdateCustomer,
        actor: &Actor,
    ) -> Result<Customer, DomainError> {
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
        log_customer_activity(&txn, &customer, ActivityAction::Updated, actor).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }

    async fn soft_delete(&self, id: &CustomerId, actor: &Actor) -> Result<Customer, DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;

        let result = CustomerEntity::update_many()
            .col_expr(
                entity::Column::DeletedAt,
                sea_orm::sea_query::Expr::current_timestamp(),
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
        log_customer_activity(&txn, &customer, ActivityAction::SoftDeleted, actor).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }

    async fn restore(&self, id: &CustomerId, actor: &Actor) -> Result<Customer, DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;

        let result = CustomerEntity::update_many()
            .col_expr(
                entity::Column::DeletedAt,
                sea_orm::sea_query::Expr::value(sea_orm::Value::ChronoDateTimeUtc(None)),
            )
            .filter(entity::Column::Id.eq(id.get()))
            .filter(entity::Column::DeletedAt.is_not_null())
            .exec(&txn)
            .await
            .map_err(sea_err)?;

        if result.rows_affected == 0 {
            return Err(DomainError::NotFound {
                entity: "customer",
                id: id.to_string(),
            });
        }

        // Re-fetch for post-trigger state (deleted_at cleared + updated_at)
        let model = CustomerEntity::find_by_id(id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "customer disappeared mid-transaction".into(),
            })?;

        let customer = Customer::from(model);
        log_customer_activity(&txn, &customer, ActivityAction::Restored, actor).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mokumo_core::actor::Actor;
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

    async fn create_test_customer(
        repo: &SeaOrmCustomerRepo,
        display_name: &str,
        company_name: Option<&str>,
        email: Option<&str>,
    ) -> Customer {
        repo.create(
            &CreateCustomer {
                display_name: display_name.to_string(),
                company_name: company_name.map(String::from),
                email: email.map(String::from),
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
            },
            &Actor::system(),
        )
        .await
        .expect("create test customer")
    }

    #[tokio::test]
    async fn list_search_filters_by_display_name() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let db_path = tmp.path().join("test.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = crate::initialize_database(&database_url)
            .await
            .expect("init db");
        let repo = SeaOrmCustomerRepo::new(pool);

        create_test_customer(
            &repo,
            "Acme Printing",
            Some("Acme Corp"),
            Some("info@acme.com"),
        )
        .await;
        create_test_customer(
            &repo,
            "Beta Apparel",
            Some("Beta LLC"),
            Some("hello@beta.com"),
        )
        .await;
        create_test_customer(&repo, "Gamma Designs", None, None).await;

        let params = PageParams::new(Some(1), Some(25));
        let filter = IncludeDeleted::ExcludeDeleted;

        // Search by display_name
        let (results, count) = repo
            .list(params, filter, Some("acme"))
            .await
            .expect("search");
        assert_eq!(count, 1);
        assert_eq!(results[0].display_name, "Acme Printing");

        // Search by company_name
        let (results, count) = repo
            .list(params, filter, Some("beta"))
            .await
            .expect("search");
        assert_eq!(count, 1);
        assert_eq!(results[0].display_name, "Beta Apparel");

        // Search by email
        let (results, count) = repo
            .list(params, filter, Some("@acme"))
            .await
            .expect("search");
        assert_eq!(count, 1);
        assert_eq!(results[0].display_name, "Acme Printing");

        // No search returns all
        let (results, count) = repo.list(params, filter, None).await.expect("no search");
        assert_eq!(count, 3);
        assert_eq!(results.len(), 3);

        // Empty search returns all
        let (results, count) = repo
            .list(params, filter, Some(""))
            .await
            .expect("empty search");
        assert_eq!(count, 3);
        assert_eq!(results.len(), 3);

        // No match
        let (_, count) = repo
            .list(params, filter, Some("zzzzz"))
            .await
            .expect("no match");
        assert_eq!(count, 0);
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
        let result = repo.create(&sample_create(), &Actor::system()).await;
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

    fn empty_update() -> UpdateCustomer {
        UpdateCustomer::default()
    }

    fn sample_create() -> CreateCustomer {
        CreateCustomer {
            display_name: "Test Corp".to_string(),
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
    async fn test_double_soft_delete_returns_not_found() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = SeaOrmCustomerRepo::new(db);

        let actor = Actor::system();
        let customer = repo.create(&sample_create(), &actor).await.unwrap();

        // First soft-delete succeeds
        repo.soft_delete(&customer.id, &actor).await.unwrap();

        // Second soft-delete should return NotFound
        let result = repo.soft_delete(&customer.id, &actor).await;
        assert!(
            matches!(result, Err(DomainError::NotFound { .. })),
            "double soft-delete should return NotFound, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_empty_update_is_noop() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = SeaOrmCustomerRepo::new(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        let before = repo
            .find_by_id(&customer.id, IncludeDeleted::ExcludeDeleted)
            .await
            .unwrap()
            .unwrap();

        // Update with all None fields — should be a no-op
        let after = repo
            .update(&customer.id, &empty_update(), &actor)
            .await
            .unwrap();

        assert_eq!(before.display_name, after.display_name);
        assert_eq!(before.company_name, after.company_name);
        assert_eq!(before.email, after.email);
        assert_eq!(before.phone, after.phone);
        assert_eq!(before.address_line1, after.address_line1);
        assert_eq!(before.address_line2, after.address_line2);
        assert_eq!(before.city, after.city);
        assert_eq!(before.state, after.state);
        assert_eq!(before.postal_code, after.postal_code);
        assert_eq!(before.country, after.country);
        assert_eq!(before.notes, after.notes);
        assert_eq!(before.portal_enabled, after.portal_enabled);
        assert_eq!(before.tax_exempt, after.tax_exempt);
        assert_eq!(before.payment_terms, after.payment_terms);
        assert_eq!(before.credit_limit_cents, after.credit_limit_cents);
        assert_eq!(before.lead_source, after.lead_source);
        assert_eq!(before.tags, after.tags);
        // updated_at intentionally not asserted — SQLite trigger fires on any
        // UPDATE, so it advances even when no business fields change.
    }

    #[tokio::test]
    async fn test_restore_clears_deleted_at() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = SeaOrmCustomerRepo::new(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        repo.soft_delete(&customer.id, &actor).await.unwrap();

        let restored = repo.restore(&customer.id, &actor).await.unwrap();
        assert!(
            restored.deleted_at.is_none(),
            "deleted_at should be cleared after restore"
        );
        assert_eq!(restored.id, customer.id);
    }

    #[tokio::test]
    async fn test_restore_non_deleted_returns_not_found() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = SeaOrmCustomerRepo::new(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();

        let result = repo.restore(&customer.id, &actor).await;
        assert!(
            matches!(result, Err(DomainError::NotFound { .. })),
            "restoring a non-deleted customer should return NotFound, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_restored_customer_appears_in_default_list() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = SeaOrmCustomerRepo::new(db);
        let actor = Actor::system();
        let params = PageParams::new(Some(1), Some(25));

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        repo.soft_delete(&customer.id, &actor).await.unwrap();

        // Should not appear in default list
        let (list, _) = repo
            .list(params, IncludeDeleted::ExcludeDeleted, None)
            .await
            .unwrap();
        assert!(
            list.iter().all(|c| c.id != customer.id),
            "deleted customer should not appear in default list"
        );

        repo.restore(&customer.id, &actor).await.unwrap();

        // Should reappear in default list
        let (list, _) = repo
            .list(params, IncludeDeleted::ExcludeDeleted, None)
            .await
            .unwrap();
        assert!(
            list.iter().any(|c| c.id == customer.id),
            "restored customer should appear in default list"
        );
    }
}
