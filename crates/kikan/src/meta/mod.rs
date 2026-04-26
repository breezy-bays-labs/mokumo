//! Install-level state stored in `meta.db`.
//!
//! `meta.db` lives at the root of the data directory alongside
//! `sessions.db`. It is the source of truth for install-wide concerns —
//! the runtime profile registry (`meta.profiles`) and the
//! cross-profile auth surface (users, roles, profile_user_roles,
//! integrations) per `adr-kikan-upgrade-migration-strategy.md` and the
//! M00 meta-DB introduction shape.

pub mod backup;
pub mod boot_state;
pub mod diagnostics;
pub mod entity;
pub mod profiles;
pub mod upgrade;

pub use backup::{
    BUNDLE_MANIFEST_SCHEMA_VERSION, BundleBackupError, BundleManifest, BundleManifestEntry,
    BundleRestoreError, DbInBundle, RestoreTarget, create_bundle, restore_bundle,
};
pub use boot_state::{AbandonReason, BootState, BootStateDetectionError, detect_boot_state};
pub use diagnostics::SidecarRecoveryDiagnostic;
pub use profiles::{Profile, ProfileRepo, ProfileRepoError, SeaOrmProfileRepo};
pub use upgrade::{UpgradeError, UpgradeOutcome, run_legacy_upgrade};
