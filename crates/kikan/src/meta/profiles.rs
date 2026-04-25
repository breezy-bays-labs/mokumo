//! `meta.profiles` domain type, repository port, and SeaORM adapter.
//!
//! This is the runtime profile registry — the install-level list of
//! profile slugs the engine can route to. Each row is a tenant in
//! kikan's multi-profile model. `kind` is opaque to kikan (its vocabulary
//! is owned by the vertical's `Graft::ProfileKind`), but kikan owns the
//! storage and lookup surface.

use sea_orm::entity::prelude::*;
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

use super::entity::profile as profile_entity;
use crate::slug::Slug;

/// Runtime profile registry entry. Construction is gated by
/// [`Profile::from_parts`] (and the repo-side conversion); fields stay
/// private so the active/archived invariant cannot be violated by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    slug: Slug,
    display_name: String,
    kind: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    archived_at: Option<OffsetDateTime>,
}

impl Profile {
    /// Construct a `Profile` from validated parts.
    pub fn from_parts(
        slug: Slug,
        display_name: String,
        kind: String,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
        archived_at: Option<OffsetDateTime>,
    ) -> Self {
        Self {
            slug,
            display_name,
            kind,
            created_at,
            updated_at,
            archived_at,
        }
    }

    pub fn slug(&self) -> &Slug {
        &self.slug
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub fn updated_at(&self) -> OffsetDateTime {
        self.updated_at
    }

    pub fn archived_at(&self) -> Option<OffsetDateTime> {
        self.archived_at
    }

    pub fn is_active(&self) -> bool {
        self.archived_at.is_none()
    }
}

/// Errors surfaced by [`ProfileRepo`].
#[derive(Debug, Error)]
pub enum ProfileRepoError {
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),

    #[error("row from `meta.profiles` has invalid slug `{slug}`: {source}")]
    InvalidSlug {
        slug: String,
        #[source]
        source: crate::slug::SlugError,
    },

    #[error("row from `meta.profiles` has invalid timestamp `{value}` in column `{column}`")]
    InvalidTimestamp { column: &'static str, value: String },

    /// A row with this slug already exists in `meta.profiles`. Returned by
    /// create paths so callers can distinguish a unique-constraint violation
    /// from other DB errors.
    #[error("profile with slug `{slug}` already exists")]
    Conflict { slug: Slug },

    /// No row in `meta.profiles` matches this slug. Returned by
    /// read/update/delete paths.
    #[error("no profile with slug `{slug}`")]
    NotFound { slug: Slug },
}

/// Port for `meta.profiles` persistence. Implementations live next to
/// this trait.
pub trait ProfileRepo: Send + Sync {
    fn list_active(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<Profile>, ProfileRepoError>> + Send;
}

/// SeaORM-backed implementation of [`ProfileRepo`] reading from the meta
/// pool.
pub struct SeaOrmProfileRepo {
    pool: DatabaseConnection,
}

impl SeaOrmProfileRepo {
    pub fn new(pool: DatabaseConnection) -> Self {
        Self { pool }
    }
}

fn parse_ts(column: &'static str, raw: String) -> Result<OffsetDateTime, ProfileRepoError> {
    OffsetDateTime::parse(&raw, &Iso8601::DEFAULT)
        .map_err(|_| ProfileRepoError::InvalidTimestamp { column, value: raw })
}

impl TryFrom<profile_entity::Model> for Profile {
    type Error = ProfileRepoError;

    fn try_from(m: profile_entity::Model) -> Result<Self, Self::Error> {
        let slug = Slug::new(m.slug.clone()).map_err(|source| ProfileRepoError::InvalidSlug {
            slug: m.slug,
            source,
        })?;
        let created_at = parse_ts("created_at", m.created_at)?;
        let updated_at = parse_ts("updated_at", m.updated_at)?;
        let archived_at = m
            .archived_at
            .map(|raw| parse_ts("archived_at", raw))
            .transpose()?;
        Ok(Profile::from_parts(
            slug,
            m.display_name,
            m.kind,
            created_at,
            updated_at,
            archived_at,
        ))
    }
}

impl ProfileRepo for SeaOrmProfileRepo {
    async fn list_active(&self) -> Result<Vec<Profile>, ProfileRepoError> {
        use profile_entity::Column;
        let rows = profile_entity::Entity::find()
            .filter(Column::ArchivedAt.is_null())
            .order_by_asc(Column::Slug)
            .all(&self.pool)
            .await?;
        rows.into_iter().map(Profile::try_from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::platform::run_platform_meta_migrations;
    use sea_orm::ConnectionTrait;

    async fn meta_pool() -> DatabaseConnection {
        let pool = crate::db::initialize_database("sqlite::memory:")
            .await
            .unwrap();
        run_platform_meta_migrations(&pool).await.unwrap();
        pool
    }

    async fn insert_profile(pool: &DatabaseConnection, slug: &str, archived_at: Option<&str>) {
        use sea_orm::Statement;
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO profiles (slug, display_name, kind, created_at, updated_at, archived_at) \
             VALUES (?, ?, 'production', '2026-04-25T00:00:00Z', '2026-04-25T00:00:00Z', ?)",
            [
                slug.into(),
                format!("{slug}-name").into(),
                archived_at.map(str::to_owned).into(),
            ],
        );
        pool.execute_raw(stmt).await.unwrap();
    }

    #[tokio::test]
    async fn list_active_filters_archived_and_orders_by_slug() {
        let pool = meta_pool().await;
        insert_profile(&pool, "zulu-printing", None).await;
        insert_profile(&pool, "alpha-shop", None).await;
        insert_profile(&pool, "ghost-shop", Some("2026-04-25T00:00:00Z")).await;
        let repo = SeaOrmProfileRepo::new(pool);
        let profiles = repo.list_active().await.unwrap();
        let slugs: Vec<&str> = profiles.iter().map(|p| p.slug().as_str()).collect();
        assert_eq!(slugs, vec!["alpha-shop", "zulu-printing"]);
        assert!(profiles.iter().all(Profile::is_active));
    }

    #[tokio::test]
    async fn list_active_returns_invalid_slug_error_on_corrupt_row() {
        let pool = meta_pool().await;
        insert_profile(&pool, "BAD-SLUG", None).await;
        let repo = SeaOrmProfileRepo::new(pool);
        let err = repo.list_active().await.unwrap_err();
        assert!(matches!(
            err,
            ProfileRepoError::InvalidSlug { ref slug, .. } if slug == "BAD-SLUG"
        ));
    }
}
