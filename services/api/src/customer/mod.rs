use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use mokumo_core::customer::service::CustomerService;
use mokumo_core::customer::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use mokumo_core::error::DomainError;
use mokumo_core::filter::IncludeDeleted;
use mokumo_db::activity::repo::SqliteActivityLogRepo;
use mokumo_db::customer::repo::SqliteCustomerRepo;
use mokumo_types::customer::CustomerResponse;
use mokumo_types::pagination::PaginatedList;
use serde::Deserialize;

use crate::SharedState;
use crate::error::AppError;
use crate::pagination::PaginationParams;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/", get(list_customers).post(create_customer))
        .route(
            "/{id}",
            get(get_customer)
                .put(update_customer)
                .delete(delete_customer),
        )
}

pub fn to_response(c: Customer) -> CustomerResponse {
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
        tax_exemption_expires_at: c.tax_exemption_expires_at,
        payment_terms: c.payment_terms,
        credit_limit_cents: c.credit_limit_cents,
        stripe_customer_id: c.stripe_customer_id,
        quickbooks_customer_id: c.quickbooks_customer_id,
        lead_source: c.lead_source,
        tags: c.tags,
        created_at: c.created_at,
        updated_at: c.updated_at,
        deleted_at: c.deleted_at,
    }
}

fn parse_customer_id(id: &str) -> Result<CustomerId, AppError> {
    id.parse().map_err(|_| {
        AppError::Domain(DomainError::NotFound {
            entity: "customer",
            id: id.to_string(),
        })
    })
}

fn customer_service(
    state: &SharedState,
) -> CustomerService<SqliteCustomerRepo, SqliteActivityLogRepo> {
    CustomerService::new(
        SqliteCustomerRepo::new(state.db.clone()),
        SqliteActivityLogRepo::new(state.db.clone()),
    )
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
}

async fn create_customer(
    State(state): State<SharedState>,
    Json(req): Json<CreateCustomer>,
) -> Result<(StatusCode, Json<CustomerResponse>), AppError> {
    let svc = customer_service(&state);
    let customer = svc.create(&req).await?;
    Ok((StatusCode::CREATED, Json(to_response(customer))))
}

async fn get_customer(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(query): Query<IncludeDeletedQuery>,
) -> Result<Json<CustomerResponse>, AppError> {
    let customer_id = parse_customer_id(&id)?;

    let svc = customer_service(&state);
    let customer = svc
        .find_by_id(&customer_id, include_deleted_filter(query.include_deleted))
        .await?
        .ok_or_else(|| {
            AppError::Domain(DomainError::NotFound {
                entity: "customer",
                id,
            })
        })?;

    Ok(Json(to_response(customer)))
}

async fn list_customers(
    State(state): State<SharedState>,
    Query(query): Query<ListCustomersQuery>,
) -> Result<Json<PaginatedList<CustomerResponse>>, AppError> {
    let filter = include_deleted_filter(query.include_deleted);
    let params = PaginationParams {
        page: query.page,
        per_page: query.per_page,
    }
    .into_page_params();

    let svc = customer_service(&state);
    let (customers, total) = svc.list(params, filter).await?;

    let items: Vec<CustomerResponse> = customers.into_iter().map(to_response).collect();
    Ok(Json(PaginatedList::new(
        items,
        total,
        params.page(),
        params.per_page(),
    )))
}

async fn update_customer(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateCustomer>,
) -> Result<Json<CustomerResponse>, AppError> {
    let customer_id = parse_customer_id(&id)?;
    let svc = customer_service(&state);
    let customer = svc.update(&customer_id, &req).await?;
    Ok(Json(to_response(customer)))
}

async fn delete_customer(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<CustomerResponse>, AppError> {
    let customer_id = parse_customer_id(&id)?;
    let svc = customer_service(&state);
    let customer = svc.soft_delete(&customer_id).await?;
    Ok(Json(to_response(customer)))
}
