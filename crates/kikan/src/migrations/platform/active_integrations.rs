//! M00 platform migration: `active_integrations` — install-level integration state.
//!
//! Integrations are install-level external-service adapters (payment
//! processors, suppliers, backup targets, etc.); unlike extensions which
//! activate per-profile, a given integration is configured once at the
//! install level and shared across profiles that are eligible (Pattern 1
//! coupling per `adr-mokumo-integrations` §ADR-1).
//!
//! Credentials are stored as `seal`-ed blobs per
//! `adr-mokumo-integrations` §ADR-5 (XChaCha20-Poly1305 via RustCrypto
//! `chacha20poly1305`); the AEAD details are opaque to this table. On
//! "disconnect and delete credentials" (T2+sudo), the `credentials_*`
//! columns are zeroed but the row remains — preserves the FK target for
//! `integration_event_log` entries, which honors the audit-trail intent
//! over the credential-deletion intent.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct ActiveIntegrations;

#[async_trait::async_trait]
impl Migration for ActiveIntegrations {
    fn name(&self) -> &'static str {
        "m20260424_000002_active_integrations"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::PerProfile
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        Vec::new()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        // `integration_id` is the trait `id()` returned by the
        // `MokumoIntegration` impl (e.g. "stripe-payments", "sanmar"); it
        // is opaque to kikan. `schema_version` tracks the plaintext-
        // envelope version so per-integration migrations can evolve
        // independently without touching the root-key file.
        //
        // `enabled_at = NULL` means the integration is configured but
        // disconnected; `credentials_ciphertext = NULL` means credentials
        // have been zeroed (the T2+sudo disconnect-and-delete path). Both
        // states coexist because the trait's lifecycle separates the two
        // intents.
        //
        // `credentials_ciphertext` and `credentials_nonce` move together —
        // either both are present (a sealed AEAD envelope) or both are
        // NULL (zeroed). The CHECK enforces this at the DB layer so a
        // half-state can't silently corrupt the AEAD invariant and only
        // surface as a decrypt failure.
        conn.execute_unprepared(
            "CREATE TABLE active_integrations (
                integration_id TEXT PRIMARY KEY,
                enabled_at TEXT,
                credentials_ciphertext BLOB,
                credentials_nonce BLOB,
                last_sync_at TEXT,
                schema_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                CHECK ((credentials_ciphertext IS NULL) = (credentials_nonce IS NULL))
            )",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TRIGGER active_integrations_updated_at
                AFTER UPDATE ON active_integrations
                FOR EACH ROW
                WHEN NEW.updated_at = OLD.updated_at
                BEGIN
                    UPDATE active_integrations
                       SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE integration_id = OLD.integration_id;
                END",
        )
        .await?;

        Ok(())
    }
}
