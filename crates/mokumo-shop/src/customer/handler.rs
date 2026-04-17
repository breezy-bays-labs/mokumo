//! HTTP handlers for the customer vertical.
//!
//! Extracts per-request DB via `kikan::ProfileDb` so every request sees
//! the database chosen by its own session (preserves seamless profile
//! switching). Router state carries only the singleton
//! `Arc<dyn kikan::ActivityWriter>` — no DB handle at router level.
//!
//! Mounted from `services/api/src/lib.rs` via
//! `.nest("/api/customers", mokumo_shop::customer::customer_router())
//!     .with_state(CustomerRouterDeps { activity_writer })`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use axum_login::AuthSession;
use kikan_types::pagination::PaginatedList;
use mokumo_core::actor::Actor;
use mokumo_core::error::DomainError;
use mokumo_core::filter::IncludeDeleted;
use mokumo_core::pagination::PageParams;
use serde::Deserialize;

use crate::customer::error::CustomerHandlerError;
use crate::customer::{
    CreateCustomer, Customer, CustomerId, CustomerService, SqliteCustomerRepository, UpdateCustomer,
};
use crate::types::CustomerResponse;

/// Singleton dependencies for the customer router.
///
/// Only platform-level singletons live here — anything per-request (DB
/// handle, authenticated user, pagination) is extracted per request.
#[derive(Clone)]
pub struct CustomerRouterDeps {
    pub activity_writer: Arc<dyn kikan::ActivityWriter>,
}

pub fn customer_router() -> Router<CustomerRouterDeps> {
    Router::new()
        .route("/", get(list_customers).post(create_customer))
        .route(
            "/{id}",
            get(get_customer)
                .put(update_customer)
                .delete(delete_customer),
        )
        .route("/{id}/restore", patch(restore_customer))
}

fn to_response(c: Customer) -> CustomerResponse {
    CustomerResponse {
        id: c.id.to_string(),
        company_name: c.company_name,
        display_name: c.display_name,
        email: c.email,
        phone: c.phone,
        address_line1: c.address_line1,
        address_line2: c.address_line2,
        city: c.city,
        state: c.state,
        postal_code: c.postal_code,
        country: c.country,
        notes: c.notes,
        portal_enabled: c.portal_enabled,
        portal_user_id: c.portal_user_id,
        tax_exempt: c.tax_exempt,
        tax_exemption_certificate_path: c.tax_exemption_certificate_path,
        tax_exemption_expires_at: c.tax_exemption_expires_at.map(|dt| dt.to_rfc3339()),
        payment_terms: c.payment_terms,
        credit_limit_cents: c.credit_limit_cents,
        stripe_customer_id: c.stripe_customer_id,
        quickbooks_customer_id: c.quickbooks_customer_id,
        lead_source: c.lead_source,
        tags: c.tags,
        created_at: c.created_at.to_rfc3339(),
        updated_at: c.updated_at.to_rfc3339(),
        deleted_at: c.deleted_at.map(|dt| dt.to_rfc3339()),
    }
}

fn parse_customer_id(id: &str) -> Result<CustomerId, CustomerHandlerError> {
    id.parse().map_err(|_| {
        CustomerHandlerError::from(DomainError::NotFound {
            entity: "customer",
            id: id.to_string(),
        })
    })
}

fn actor_from_session(auth_session: &AuthSession<kikan::auth::Backend>) -> Actor {
    match &auth_session.user {
        Some(user) => Actor::user(user.user.id.get()),
        None => Actor::system(),
    }
}

fn build_service(
    db: kikan::db::DatabaseConnection,
    deps: &CustomerRouterDeps,
) -> CustomerService<SqliteCustomerRepository> {
    CustomerService::new(SqliteCustomerRepository::new(
        db,
        deps.activity_writer.clone(),
    ))
}

fn include_deleted_filter(flag: Option<bool>) -> IncludeDeleted {
    if flag.unwrap_or(false) {
        IncludeDeleted::IncludeDeleted
    } else {
        IncludeDeleted::ExcludeDeleted
    }
}

#[derive(Deserialize)]
struct IncludeDeletedQuery {
    include_deleted: Option<bool>,
}

#[derive(Deserialize)]
struct ListCustomersQuery {
    include_deleted: Option<bool>,
    page: Option<u32>,
    per_page: Option<u32>,
    search: Option<String>,
}

async fn create_customer(
    auth_session: AuthSession<kikan::auth::Backend>,
    kikan::ProfileDb(db): kikan::ProfileDb,
    State(deps): State<CustomerRouterDeps>,
    Json(req): Json<CreateCustomer>,
) -> Result<(StatusCode, Json<CustomerResponse>), CustomerHandlerError> {
    let actor = actor_from_session(&auth_session);
    let svc = build_service(db, &deps);
    let customer = svc.create(&req, &actor).await?;
    Ok((StatusCode::CREATED, Json(to_response(customer))))
}

async fn get_customer(
    kikan::ProfileDb(db): kikan::ProfileDb,
    State(deps): State<CustomerRouterDeps>,
    Path(id): Path<String>,
    Query(query): Query<IncludeDeletedQuery>,
) -> Result<Json<CustomerResponse>, CustomerHandlerError> {
    let customer_id = parse_customer_id(&id)?;
    let svc = build_service(db, &deps);
    let customer = svc
        .find_by_id(&customer_id, include_deleted_filter(query.include_deleted))
        .await?
        .ok_or_else(|| {
            CustomerHandlerError::from(DomainError::NotFound {
                entity: "customer",
                id,
            })
        })?;
    Ok(Json(to_response(customer)))
}

async fn list_customers(
    kikan::ProfileDb(db): kikan::ProfileDb,
    State(deps): State<CustomerRouterDeps>,
    Query(query): Query<ListCustomersQuery>,
) -> Result<Json<PaginatedList<CustomerResponse>>, CustomerHandlerError> {
    let filter = include_deleted_filter(query.include_deleted);
    let params = PageParams::new(query.page, query.per_page);

    let svc = build_service(db, &deps);
    let (customers, total) = svc.list(params, filter, query.search.as_deref()).await?;

    let items: Vec<CustomerResponse> = customers.into_iter().map(to_response).collect();
    Ok(Json(PaginatedList::new(
        items,
        total,
        params.page(),
        params.per_page(),
    )))
}

async fn update_customer(
    auth_session: AuthSession<kikan::auth::Backend>,
    kikan::ProfileDb(db): kikan::ProfileDb,
    State(deps): State<CustomerRouterDeps>,
    Path(id): Path<String>,
    Json(req): Json<UpdateCustomer>,
) -> Result<Json<CustomerResponse>, CustomerHandlerError> {
    let actor = actor_from_session(&auth_session);
    let customer_id = parse_customer_id(&id)?;
    let svc = build_service(db, &deps);
    let customer = svc.update(&customer_id, &req, &actor).await?;
    Ok(Json(to_response(customer)))
}

async fn delete_customer(
    auth_session: AuthSession<kikan::auth::Backend>,
    kikan::ProfileDb(db): kikan::ProfileDb,
    State(deps): State<CustomerRouterDeps>,
    Path(id): Path<String>,
) -> Result<Json<CustomerResponse>, CustomerHandlerError> {
    let actor = actor_from_session(&auth_session);
    let customer_id = parse_customer_id(&id)?;
    let svc = build_service(db, &deps);
    let customer = svc.soft_delete(&customer_id, &actor).await?;
    Ok(Json(to_response(customer)))
}

async fn restore_customer(
    auth_session: AuthSession<kikan::auth::Backend>,
    kikan::ProfileDb(db): kikan::ProfileDb,
    State(deps): State<CustomerRouterDeps>,
    Path(id): Path<String>,
) -> Result<Json<CustomerResponse>, CustomerHandlerError> {
    let actor = actor_from_session(&auth_session);
    let customer_id = parse_customer_id(&id)?;
    let svc = build_service(db, &deps);
    let customer = svc.restore(&customer_id, &actor).await?;
    Ok(Json(to_response(customer)))
}
