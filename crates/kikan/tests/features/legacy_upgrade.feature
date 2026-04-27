Feature: Legacy install upgrade — data move + state machine

  Pre-PR-A per-profile DBs hold a `users` and `roles` table that the
  post-PR-A schema places on `meta.db`. The legacy upgrade migrates
  those rows across once per install, and is self-healing across
  power loss / crash / partial commit. Multi-legacy-profile installs
  and external mutation of meta.db are detected and refused before
  any data is mutated.

  # --- State A: fresh upgrade ---

  Scenario: State A — pre-PR-A install migrates users and drops legacy tables
    Given a meta DB with platform migrations applied
    And a per-profile DB with legacy users and roles tables and one admin
    When I run the legacy upgrade
    Then the upgrade succeeds
    And meta.users contains 1 user
    And meta.roles contains 3 platform-seeded roles
    And the per-profile DB has no legacy users table
    And the per-profile DB has no legacy roles table
    And the meta legacy_upgrade_locks table contains 1 row

  Scenario: State A — pre-Stage-3 fixture upgrades end-to-end via the on-disk SQLite file
    Given a meta DB with platform migrations applied
    And a per-profile DB seeded from the pre-Stage-3 fixture
    When I run the legacy upgrade
    Then the upgrade succeeds
    And meta.users contains at least 1 user
    And the per-profile DB has no legacy users table

  # --- State B: crash recovery ---

  Scenario: State B — meta already has legacy emails so only the drop runs
    Given a meta DB with platform migrations applied
    And meta.users already contains the legacy admin row
    And a per-profile DB with legacy users and roles tables and one admin
    When I run the legacy upgrade
    Then the upgrade succeeds
    And meta.users contains 1 user
    And the per-profile DB has no legacy users table
    And the meta legacy_upgrade_locks table contains 0 rows

  # --- State C: completed / no-op ---

  Scenario: State C — per-profile DB has no legacy users table
    Given a meta DB with platform migrations applied
    And a per-profile DB with no legacy users table
    When I run the legacy upgrade
    Then the upgrade succeeds
    And meta.users contains 0 users
    And the meta legacy_upgrade_locks table contains 0 rows

  # --- Multi-legacy-profile collisions: graceful abort ---

  Scenario: User-id collision aborts before any meta mutation
    Given a meta DB with platform migrations applied
    And meta.users already contains a foreign user at id 1
    And a per-profile DB with legacy users and roles tables and one admin
    When I run the legacy upgrade
    Then the upgrade fails with UnsupportedLegacyState mentioning "legacy user IDs"
    And the per-profile DB still has the legacy users table
    And meta.profiles contains 0 rows
    And the meta legacy_upgrade_locks table contains 0 rows

  Scenario: Custom role-id collision aborts with privilege escalation warning
    Given a meta DB with platform migrations applied
    And meta.roles already contains a foreign role at id 4
    And a per-profile DB with legacy users and roles tables and a custom role at id 4
    When I run the legacy upgrade
    Then the upgrade fails with UnsupportedLegacyState mentioning "privilege escalation"
    And the per-profile DB still has the legacy users table
    And meta.profiles contains 0 rows

  # --- Anomalous partial intersection ---

  Scenario: Partial email intersection aborts with anomalous-state diagnostic
    Given a meta DB with platform migrations applied
    And a per-profile DB with two legacy admins
    And meta.users contains only the first legacy admin's email
    When I run the legacy upgrade
    Then the upgrade fails with UnsupportedLegacyState mentioning "external mutation"
    And the per-profile DB still has the legacy users table
    And meta.profiles contains 0 rows

  # --- Schema-compatibility union ---

  Scenario: Schema-compat check accepts platform-owned legacy seaql_migrations rows
    Given a per-profile SQLite file with seaql_migrations rows for users_and_roles and shop_settings
    When I check schema compatibility against a vertical migrator that does not declare those rows
    Then the schema-compat check passes
