use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use kikan_types::activity::{ActivityEntryResponse, to_response};
use kikan_types::pagination::PaginatedList;
use mokumo_core::activity::traits::ActivityLogRepository;
use mokumo_db::activity::repo::SqliteActivityLogRepo;
use serde::Deserialize;

use crate::SharedState;
use crate::error::AppError;
use crate::pagination::PaginationParams;
use kikan::ProfileDb;

pub fn router() -> Router<SharedState> {
    Router::new().route("/", get(list_activity))
}

#[derive(Deserialize)]
struct ListActivityQuery {
    entity_type: Option<String>,
    entity_id: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn list_activity(
    ProfileDb(db): ProfileDb,
    Query(query): Query<ListActivityQuery>,
) -> Result<Json<PaginatedList<ActivityEntryResponse>>, AppError> {
    let params = PaginationParams {
        page: query.page,
        per_page: query.per_page,
    }
    .into_page_params();

    let repo = SqliteActivityLogRepo::new(db.get_sqlite_connection_pool().clone());
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
