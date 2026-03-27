use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use mokumo_core::activity::ActivityEntry;
use mokumo_core::activity::traits::ActivityLogRepository;
use mokumo_db::activity::repo::SqliteActivityLogRepo;
use mokumo_types::activity::ActivityEntryResponse;
use mokumo_types::pagination::PaginatedList;
use serde::Deserialize;

use crate::SharedState;
use crate::error::AppError;
use crate::pagination::PaginationParams;

pub fn router() -> Router<SharedState> {
    Router::new().route("/", get(list_activity))
}

pub fn to_response(e: ActivityEntry) -> ActivityEntryResponse {
    ActivityEntryResponse {
        id: e.id,
        entity_type: e.entity_type,
        entity_id: e.entity_id,
        action: e.action,
        actor_id: e.actor_id,
        actor_type: e.actor_type,
        payload: Some(e.payload),
        created_at: e.created_at.to_rfc3339(),
    }
}

#[derive(Deserialize)]
struct ListActivityQuery {
    entity_type: Option<String>,
    entity_id: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn list_activity(
    State(state): State<SharedState>,
    Query(query): Query<ListActivityQuery>,
) -> Result<Json<PaginatedList<ActivityEntryResponse>>, AppError> {
    let params = PaginationParams {
        page: query.page,
        per_page: query.per_page,
    }
    .into_page_params();

    let repo = SqliteActivityLogRepo::new(state.db.get_sqlite_connection_pool().clone());
    let (entries, total) = repo
        .list(
            query.entity_type.as_deref(),
            query.entity_id.as_deref(),
            params,
        )
        .await?;

    let items: Vec<ActivityEntryResponse> = entries.into_iter().map(to_response).collect();
    Ok(Json(PaginatedList::new(
        items,
        total,
        params.page(),
        params.per_page(),
    )))
}
