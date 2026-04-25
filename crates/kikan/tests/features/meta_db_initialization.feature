Feature: Meta DB initialization

  On first boot, kikan opens (creating if absent) a process-wide
  `meta.db` at the data-directory top level alongside `sessions.db`.
  Bootstrap tables are stamped on it before any Meta-target migration
  runs, and the new `meta.profiles` table is created by the first
  platform Meta migration.

  Engine-platform migrations (users, roles, profile_user_roles,
  prevent_last_admin_deactivation, active_integrations,
  integration_event_log) are recorded in `meta.db.kikan_migrations`
  with `graft_id = 'kikan::engine'`. Per-database history per
  `adr-kikan-upgrade-migration-strategy.md`.

  Background:
    Given a fresh data directory with no meta.db
    When the engine boots

  Scenario: meta.db file is created on first boot
    Then a meta.db file exists at the data directory top level

  Scenario: bootstrap tables are stamped on meta.db
    Then meta.db contains a kikan_migrations table
    And meta.db contains a kikan_meta table

  Scenario: meta.profiles table is created by the first platform Meta migration
    Then meta.db contains a profiles table

  Scenario: engine-platform migrations are recorded in meta.db with graft_id 'kikan::engine'
    Then meta.db kikan_migrations records the engine-platform migrations under graft_id "kikan::engine"
