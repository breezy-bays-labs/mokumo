//! SQLite adapter for `CustomerRepository`.
//!
//! Mutations run inside a SeaORM transaction; activity-log persistence
//! delegates to the injected `kikan::ActivityWriter`, which performs its
//! INSERT on the same `DatabaseTransaction` for atomicity.

use std::sync::Arc;

use chrono::Utc;
use kikan::activity::{ActivityLogEntry, ActivityWriter};
use kikan::actor::Actor;
use kikan::error::ActivityWriteError;
use kikan::error::DomainError;
use kikan::filter::IncludeDeleted;
use kikan::pagination::PageParams;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, DatabaseTransaction,
    EntityTrait, IntoActiveValue, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    TransactionTrait,
};

use super::CustomerRepository;
use super::domain::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use super::entity::{self, Entity as CustomerEntity};
use crate::activity::ActivityAction;

fn sea_err(e: sea_orm::DbErr) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

fn activity_err(e: ActivityWriteError) -> DomainError {
    DomainError::Internal {
        message: format!("activity log write failed: {e}"),
    }
}

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

pub struct SqliteCustomerRepository {
    db: DatabaseConnection,
    activity_writer: Arc<dyn ActivityWriter>,
}

impl SqliteCustomerRepository {
    pub fn new(db: DatabaseConnection, activity_writer: Arc<dyn ActivityWriter>) -> Self {
        Self {
            db,
            activity_writer,
        }
    }

    async fn log_activity(
        &self,
        tx: &DatabaseTransaction,
        customer: &Customer,
        action: ActivityAction,
        actor: &Actor,
    ) -> Result<(), DomainError> {
        let payload = serde_json::to_value(customer).map_err(|e| DomainError::Internal {
            message: format!("failed to serialize customer for activity log: {e}"),
        })?;
        let entry = ActivityLogEntry {
            actor_id: Some(actor.id().to_string()),
            actor_type: actor.actor_type().to_string(),
            entity_kind: "customer".to_string(),
            entity_id: customer.id.to_string(),
            action: action.as_str().to_string(),
            payload,
            occurred_at: Utc::now(),
        };
        self.activity_writer
            .log(tx, entry)
            .await
            .map_err(activity_err)
    }
}

impl CustomerRepository for SqliteCustomerRepository {
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

        let count =
            i64::try_from(base.clone().count(&self.db).await.map_err(sea_err)?).unwrap_or(i64::MAX);

        let models = base
            .order_by_desc(entity::Column::CreatedAt)
            .order_by_desc(entity::Column::Id)
            .limit(Some(u64::from(params.per_page())))
            .offset(Some(u64::from(params.offset())))
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

        self.log_activity(&txn, &customer, ActivityAction::Created, actor)
            .await?;

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

        let mut active = entity::ActiveModel {
            id: ActiveValue::Unchanged(id.get()),
            ..Default::default()
        };

        if let Some(ref name) = req.display_name {
            active.display_name = ActiveValue::Set(name.clone());
        }
        if let Some(v) = req.portal_enabled {
            active.portal_enabled = ActiveValue::Set(v);
        }
        if let Some(v) = req.tax_exempt {
            active.tax_exempt = ActiveValue::Set(v);
        }

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

        let model = CustomerEntity::find_by_id(id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "customer disappeared mid-transaction".into(),
            })?;

        let customer = Customer::from(model);
        self.log_activity(&txn, &customer, ActivityAction::Updated, actor)
            .await?;
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

        let model = CustomerEntity::find_by_id(id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "customer disappeared mid-transaction".into(),
            })?;

        let customer = Customer::from(model);
        self.log_activity(&txn, &customer, ActivityAction::SoftDeleted, actor)
            .await?;
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

        let model = CustomerEntity::find_by_id(id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "customer disappeared mid-transaction".into(),
            })?;

        let customer = Customer::from(model);
        self.log_activity(&txn, &customer, ActivityAction::Restored, actor)
            .await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(customer)
    }
}

#[cfg(test)]
mod tests {
    use super::super::domain::{CreateCustomer, UpdateCustomer};
    use super::*;
    use kikan::activity::SqliteActivityWriter;

    async fn test_db() -> (
        DatabaseConnection,
        sqlx::sqlite::SqlitePool,
        tempfile::TempDir,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = crate::db::initialize_database(&url).await.unwrap();
        let pool = db.get_sqlite_connection_pool().clone();
        (db, pool, tmp)
    }

    fn make_repo(db: DatabaseConnection) -> SqliteCustomerRepository {
        SqliteCustomerRepository::new(db, Arc::new(SqliteActivityWriter::new()))
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

    fn empty_update() -> UpdateCustomer {
        UpdateCustomer::default()
    }

    async fn create_test_customer(
        repo: &SqliteCustomerRepository,
        display_name: &str,
        company_name: Option<&str>,
        email: Option<&str>,
    ) -> Customer {
        repo.create(
            &CreateCustomer {
                display_name: display_name.to_string(),
                company_name: company_name.map(String::from),
                email: email.map(String::from),
                ..sample_create()
            },
            &Actor::system(),
        )
        .await
        .expect("create test customer")
    }

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
        }

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn list_search_filters_by_display_name() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);

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

        let (results, count) = repo.list(params, filter, Some("acme")).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(results[0].display_name, "Acme Printing");

        let (results, count) = repo.list(params, filter, Some("beta")).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(results[0].display_name, "Beta Apparel");

        let (results, count) = repo.list(params, filter, Some("@acme")).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(results[0].display_name, "Acme Printing");

        let (_, count) = repo.list(params, filter, None).await.unwrap();
        assert_eq!(count, 3);

        let (_, count) = repo.list(params, filter, Some("")).await.unwrap();
        assert_eq!(count, 3);

        let (_, count) = repo.list(params, filter, Some("zzzzz")).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn create_rolls_back_when_activity_log_fails() {
        let (db, pool, _tmp) = test_db().await;

        sqlx::query("DROP TABLE activity_log")
            .execute(&pool)
            .await
            .unwrap();

        let repo = make_repo(db);
        let result = repo.create(&sample_create(), &Actor::system()).await;
        assert!(
            result.is_err(),
            "create should fail when activity_log missing"
        );

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "row must roll back on activity_log failure");
    }

    #[tokio::test]
    async fn double_soft_delete_returns_not_found() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        repo.soft_delete(&customer.id, &actor).await.unwrap();

        let result = repo.soft_delete(&customer.id, &actor).await;
        assert!(matches!(result, Err(DomainError::NotFound { .. })));
    }

    #[tokio::test]
    async fn empty_update_is_noop() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        let before = repo
            .find_by_id(&customer.id, IncludeDeleted::ExcludeDeleted)
            .await
            .unwrap()
            .unwrap();

        let after = repo
            .update(&customer.id, &empty_update(), &actor)
            .await
            .unwrap();

        assert_eq!(before.display_name, after.display_name);
        assert_eq!(before.company_name, after.company_name);
        assert_eq!(before.email, after.email);
        assert_eq!(before.phone, after.phone);
        assert_eq!(before.portal_enabled, after.portal_enabled);
        assert_eq!(before.tax_exempt, after.tax_exempt);
        assert_eq!(before.payment_terms, after.payment_terms);
        assert_eq!(before.credit_limit_cents, after.credit_limit_cents);
    }

    #[tokio::test]
    async fn update_modifies_display_name() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        let after = repo
            .update(
                &customer.id,
                &UpdateCustomer {
                    display_name: Some("Renamed Corp".to_string()),
                    ..UpdateCustomer::default()
                },
                &actor,
            )
            .await
            .unwrap();

        assert_eq!(after.display_name, "Renamed Corp");
    }

    #[tokio::test]
    async fn update_on_missing_customer_returns_not_found() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();

        let result = repo
            .update(&CustomerId::generate(), &empty_update(), &actor)
            .await;
        assert!(matches!(result, Err(DomainError::NotFound { .. })));
    }

    #[tokio::test]
    async fn restore_clears_deleted_at() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        repo.soft_delete(&customer.id, &actor).await.unwrap();

        let restored = repo.restore(&customer.id, &actor).await.unwrap();
        assert!(restored.deleted_at.is_none());
        assert_eq!(restored.id, customer.id);
    }

    #[tokio::test]
    async fn restore_non_deleted_returns_not_found() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        let result = repo.restore(&customer.id, &actor).await;
        assert!(matches!(result, Err(DomainError::NotFound { .. })));
    }

    #[tokio::test]
    async fn restored_customer_appears_in_default_list() {
        let (db, _pool, _tmp) = test_db().await;
        let repo = make_repo(db);
        let actor = Actor::system();
        let params = PageParams::new(Some(1), Some(25));

        let customer = repo.create(&sample_create(), &actor).await.unwrap();
        repo.soft_delete(&customer.id, &actor).await.unwrap();

        let (list, _) = repo
            .list(params, IncludeDeleted::ExcludeDeleted, None)
            .await
            .unwrap();
        assert!(list.iter().all(|c| c.id != customer.id));

        repo.restore(&customer.id, &actor).await.unwrap();

        let (list, _) = repo
            .list(params, IncludeDeleted::ExcludeDeleted, None)
            .await
            .unwrap();
        assert!(list.iter().any(|c| c.id == customer.id));
    }
}
