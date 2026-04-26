//! Activity log HTTP handler.
//!
//! Exposes `GET /api/activity` with optional `entity_type`, `entity_id`,
//! `page`, and `per_page` filters. Uses per-request extractors only
//! (`ProfileDb`), so the router is generic over state — no
//! `ActivityRouterDeps` struct is required.
//!
//! Distinct name from [`crate::activity`] (the writer/repo) to avoid module
//! collision inside kikan.

use crate::activity::traits::ActivityLogRepository;
use crate::pagination::PageParams;
use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use kikan_types::activity::{ActivityEntryResponse, to_response};
use kikan_types::pagination::PaginatedList;
use serde::Deserialize;

use crate::AppError;
use crate::ProfileDb;
use crate::activity::SqliteActivityLogRepo;

pub fn activity_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
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
    let params = PageParams::new(query.page, query.per_page);

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
