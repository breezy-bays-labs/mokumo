Feature: Profile sidecar self-repair via Graft hook

  When the engine boots and a non-setup-wizard profile kind has a
  missing or corrupt database file, it consults the vertical's
  `Graft::recover_profile_sidecar` hook. On a successful recovery the
  engine writes a `meta.activity_log` audit entry, surfaces a
  diagnostic on `PlatformState::sidecar_recoveries`, and continues
  boot. Failures are logged and never block startup.

  Healthy installs report nothing — the diagnostic map is empty after a
  clean boot. Verticals that don't bundle a sidecar see their default
  hook impl return `NotSupported`, which the engine logs at debug and
  skips.

  See `crates/kikan/src/graft.rs` (`recover_profile_sidecar`),
  `crates/kikan/src/engine.rs` (`maybe_repair_profile_sidecars`), and
  `adr-database-startup-guard-chain.md` 2026-04-16 errata.

  Scenario: missing demo database is recovered from the bundled sidecar
    Given a fresh data directory with a bundled demo sidecar but no demo database file
    When the engine boots from the fresh data directory
    Then the demo database file exists
    And PlatformState reports a sidecar recovery for "demo"
    And meta.activity_log has a profile_sidecar_recovered entry for "demo"

  Scenario: healthy demo database produces no recovery entry
    Given a fresh data directory with a bundled demo sidecar and a healthy demo database file
    When the engine boots from the fresh data directory
    Then PlatformState reports no sidecar recoveries
