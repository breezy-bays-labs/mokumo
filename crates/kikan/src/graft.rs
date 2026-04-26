use std::path::{Path, PathBuf};

use crate::control_plane::{PinId, SetupTokenSource};
use crate::engine::EngineContext;
use crate::error::EngineError;
use crate::migrations::bootstrap::BootstrapMigrations;
use crate::migrations::platform::PlatformMigrations;
use crate::migrations::{GraftId, Migration};
use crate::tenancy::ProfileId;

#[trait_variant::make(Send)]
pub trait Graft: Sized + 'static {
    type AppState: Clone + Send + Sync + 'static;
    type DomainState: Clone + Send + Sync + 'static;

    /// The vertical's profile discriminator (e.g. a two-variant
    /// `Demo`/`Production` enum).
    ///
    /// Kikan stores and routes `ProfileKind` opaquely. The `Display` +
    /// `FromStr` pair is the single source of truth for the on-disk
    /// directory name of each kind: kikan persists the active profile as
    /// `kind.to_string()` and recovers it via `K::from_str(dir)`. A graft
    /// whose `Display` and `FromStr` impls are not inverses will fail pool
    /// lookup at request time — this is an invariant, not a style point.
    type ProfileKind: Copy
        + Eq
        + std::hash::Hash
        + Send
        + Sync
        + 'static
        + std::fmt::Display
        + std::fmt::Debug
        + std::str::FromStr<Err = String>
        + serde::Serialize
        + serde::de::DeserializeOwned;

    fn id() -> GraftId;
    fn migrations(&self) -> Vec<Box<dyn Migration>>;

    /// Filename of the per-profile SQLite database (vertical-declared,
    /// e.g. `"{vertical}.db"`). Kikan composes on-disk paths as
    /// `data_dir/{kind.to_string()}/{db_filename}`.
    fn db_filename(&self) -> &'static str;

    /// Every profile kind the vertical recognizes.
    ///
    /// Used by platform handlers that enumerate profiles (backup listing,
    /// diagnostics). Returning a `'static` slice lets kikan iterate
    /// without allocation or generics leakage.
    fn all_profile_kinds(&self) -> &'static [Self::ProfileKind];

    /// The profile kind to fall back to when the on-disk
    /// `active_profile` file is missing or unparseable.
    fn default_profile_kind(&self) -> Self::ProfileKind;

    /// Whether a profile kind needs the vertical's setup wizard before
    /// it can serve user traffic. Kikan reads this to gate
    /// `is_setup_complete` without matching on concrete variants.
    fn requires_setup_wizard(&self, kind: &Self::ProfileKind) -> bool;

    /// The profile kind that credentialed auth runs against.
    ///
    /// Kikan's `Backend::authenticate` path always hits one pool — the
    /// vertical picks which. Returning the kind here, rather than a
    /// dir-name, lets kikan build `Backend<K>` without naming variants.
    fn auth_profile_kind(&self) -> Self::ProfileKind;

    /// Build the domain-specific slice of the application state.
    ///
    /// Called by `Engine::boot()` after platform and control-plane state
    /// are constructed. The returned `DomainState` is passed to
    /// `compose_state` to assemble the full `AppState`.
    async fn build_domain_state(
        &self,
        ctx: &EngineContext,
    ) -> Result<Self::DomainState, EngineError>;

    /// Assemble the full application state from control-plane + domain slices.
    ///
    /// `control_plane` already embeds `PlatformState` (via its `platform` field),
    /// so a separate `platform` parameter is unnecessary.
    fn compose_state(
        control_plane: crate::ControlPlaneState,
        domain: Self::DomainState,
    ) -> Self::AppState;

    /// Extract the platform state slice from the composed application state.
    fn platform_state(state: &Self::AppState) -> &crate::PlatformState;

    /// Extract the control-plane state slice from the composed application state.
    fn control_plane_state(state: &Self::AppState) -> &crate::ControlPlaneState;

    fn data_plane_routes(state: &Self::AppState) -> axum::Router<Self::AppState>;

    // ── Lifecycle hooks (sync, default no-ops) ──────────────────────────

    /// Called after a backup archive has been created. Domain grafts can
    /// copy additional artifacts (e.g. logo files) into the backup.
    fn on_backup_created(&self, _db_path: &Path, _backup_path: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Called before a restore operation begins. Domain grafts can validate
    /// or prepare domain-specific state before the database is replaced.
    fn on_pre_restore(&self, _db_path: &Path, _backup_path: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Called after a restore operation completes. Domain grafts can restore
    /// additional artifacts (e.g. logo files) from the backup.
    fn on_post_restore(&self, _db_path: &Path, _backup_path: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Called after a database reset. Domain grafts can clean up
    /// domain-specific artifacts from the profile directory and any
    /// vertical-specific adjacent directories (e.g. Mokumo's
    /// file-drop recovery dir) that the graft resolves internally.
    fn on_post_reset_db(&self, _profile_dir: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Spawn domain-specific background tasks (e.g. periodic IP refresh,
    /// PIN sweep). Called once after state construction during boot.
    fn spawn_background_tasks(&self, _state: &Self::AppState) {}

    // ── Recovery / bootstrap vocabulary hooks ────────────────────────
    //
    // Three vertical-vocabulary hooks added in Session 3 per
    // `adr-kikan-engine-vocabulary` § "Amendment 2026-04-22 (b)". Each
    // ships with an empty/disabled default so verticals that do not use
    // a file-drop recovery flow or a setup-wizard token compile
    // unchanged.

    /// Filesystem directory the engine (or a kikan satellite like
    /// `kikan-cli`) may consult for vertical-specific recovery artifacts
    /// (e.g. file-drop password reset). Returning `None` disables the
    /// file-drop recovery flow for verticals that don't use it.
    ///
    /// Kikan does not read or write files at this path on its own. It
    /// only exposes the path to callers — the vertical owns the file
    /// layout and lifecycle.
    fn recovery_dir(&self, _profile_id: &ProfileId<Self::ProfileKind>) -> Option<PathBuf> {
        None
    }

    /// Source of the first-admin bootstrap token.
    ///
    /// [`SetupTokenSource::Disabled`] means the vertical does not use a
    /// setup-wizard token; the engine records `setup_token = None` and
    /// every call into `setup_admin` rejects with `PermissionDenied`
    /// (or the vertical routes around `setup_admin` entirely).
    ///
    /// `File` paths are read synchronously at boot; I/O failure surfaces
    /// as [`EngineError::Boot`]. `Inline(Arc<str>)` is cloned.
    fn setup_token_source(&self) -> SetupTokenSource {
        SetupTokenSource::Disabled
    }

    /// PIN identifiers the vertical considers valid for the file-drop
    /// reset flow. Empty slice = no PIN-based reset is offered.
    ///
    /// The PIN *values* never enter kikan; this is identifier metadata
    /// only. The vertical owns the PIN-value store (hashing, expiry,
    /// lookup) on its side of the seam.
    fn valid_reset_pin_ids(&self) -> &'static [PinId] {
        const EMPTY: &[PinId] = &[];
        EMPTY
    }

    // ── Profile sidecar recovery ─────────────────────────────────────
    //
    // Verticals may bundle a seed database alongside the binary (a
    // "sidecar") to back profile kinds that ship with pre-populated data
    // — preview installs, training data sets, and similar non-blank
    // profiles. The engine offers each non-setup-wizard profile a chance
    // to self-repair from its sidecar at boot, so a corrupted or missing
    // file can be silently recreated without operator intervention.
    //
    // Verticals that don't ship a sidecar leave the default impl alone
    // (which returns `Err(SidecarRecoveryError::NotSupported)`); the
    // engine logs at debug level and skips that kind. The kind argument
    // lets a multi-kind vertical attach a sidecar to one kind and not
    // another.

    /// Inspect the on-disk database for `kind` and recover from the
    /// vertical's bundled sidecar if the database is missing or fails
    /// integrity guards. Called once per boot for every kind where
    /// [`Self::requires_setup_wizard`] returns `false`.
    ///
    /// Implementations must be idempotent — a healthy database returns
    /// [`SidecarRecovery::NotNeeded`] without I/O on the database itself,
    /// while a failed integrity check triggers a force-copy from the
    /// sidecar source and returns [`SidecarRecovery::Recreated`].
    ///
    /// Verticals without a bundled sidecar leave the default in place;
    /// the engine treats [`SidecarRecoveryError::NotSupported`] as
    /// "nothing to do here" and logs at debug level.
    fn recover_profile_sidecar(
        &self,
        _kind: &Self::ProfileKind,
        _data_dir: &Path,
        _db_filename: &'static str,
    ) -> Result<SidecarRecovery, SidecarRecoveryError> {
        Err(SidecarRecoveryError::NotSupported)
    }

    // ── Data plane composition ───────────────────────────────────────

    /// Optional SPA fallback source. Returning `Some(…)` installs the
    /// source's router as the data-plane fallback inside
    /// [`Engine::build_router`](crate::Engine::build_router), so
    /// non-API paths serve the SPA instead of 404-ing.
    ///
    /// Kikan stays rust-embed-free; the embedding (or disk-reading)
    /// strategy lives in a sister crate such as `kikan-spa-sveltekit`
    /// that the vertical pulls in at its edge. Consumers that don't
    /// serve an SPA (headless tools, tests, CLIs) return `None` and
    /// non-API requests fall through to Axum's default 404.
    ///
    /// Called once at engine construction ([`Engine::new_with`]); the
    /// returned box is cached and consumed in [`Engine::build_router`].
    fn spa_source(&self) -> Option<Box<dyn crate::data_plane::spa::SpaSource>> {
        None
    }
}

/// Outcome of [`Graft::recover_profile_sidecar`] on success.
///
/// The engine writes a `meta.activity_log` audit entry and surfaces a
/// diagnostic on `PlatformState::sidecar_recoveries` only for the
/// `Recreated` arm — `NotNeeded` is the silent healthy path and produces
/// no map entry.
#[derive(Debug, Clone)]
pub enum SidecarRecovery {
    /// The on-disk database for this kind is healthy; no copy was made.
    NotNeeded,
    /// The on-disk database was missing or failed integrity guards and
    /// was replaced with a fresh copy from the bundled sidecar.
    Recreated {
        source: PathBuf,
        dest: PathBuf,
        bytes: u64,
    },
}

/// Failure modes for [`Graft::recover_profile_sidecar`].
#[derive(Debug, thiserror::Error)]
pub enum SidecarRecoveryError {
    /// The vertical does not bundle a sidecar for this kind. Default
    /// impl on [`Graft::recover_profile_sidecar`] returns this; the
    /// engine logs at debug level and skips the kind.
    #[error("vertical does not bundle a sidecar for this profile kind")]
    NotSupported,
    /// I/O or integrity-check failure during the recovery attempt. The
    /// engine logs the error and continues boot — recovery is best-effort
    /// and never blocks startup.
    #[error("sidecar recovery failed: {message}")]
    Failed { message: String },
}

#[async_trait::async_trait]
pub trait SubGraft: Send + Sync + 'static {
    fn id(&self) -> GraftId;
    fn migrations(&self) -> Vec<Box<dyn Migration>>;

    async fn on_ignite(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn on_liftoff(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn on_shutdown(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn check(&self) -> Result<(), EngineError> {
        Ok(())
    }
}

pub struct SelfGraft;

#[async_trait::async_trait]
impl SubGraft for SelfGraft {
    fn id(&self) -> GraftId {
        BootstrapMigrations::graft_id()
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        let mut migrations = BootstrapMigrations::migrations();
        migrations.extend(PlatformMigrations::migrations());
        migrations
    }
}
