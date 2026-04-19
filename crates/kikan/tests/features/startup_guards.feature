@future
Feature: Startup Guard Chain

  Before running migrations, Mokumo verifies the database passes a
  series of integrity checks. Each guard runs in order; failure at
  any step prevents the upgrade from proceeding.

  # --- Application identity ---

  Scenario: Database with valid Mokumo application ID passes identity check
    Given a database with application_id 0x4D4B4D4F
    When the startup guard chain runs
    Then the identity check passes

  Scenario: Database with default application ID passes identity check
    Given a database with application_id 0
    When the startup guard chain runs
    Then the identity check passes

  Scenario: Database with foreign application ID fails identity check
    Given a database with application_id 0xDEADBEEF
    When the startup guard chain runs
    Then startup fails with an invalid application ID error

  # --- Pre-migration backup ---

  Scenario: Database is backed up before upgrade
    Given an existing database at schema version 3
    When the startup guard chain runs
    Then a backup file is created before any migration executes

  Scenario: Only the three most recent backups are kept
    Given four backup files from previous upgrades
    When the startup guard chain runs
    Then a new backup is created
    And the oldest backup is removed
    And three backups remain

  Scenario: No backup is created for a new database
    Given no database file exists
    When the startup guard chain runs
    Then no backup file is created

  # --- Auto-vacuum ---

  Scenario: Auto-vacuum is configured on first upgrade
    Given a database with auto_vacuum set to NONE
    When the startup guard chain runs
    Then auto_vacuum is set to INCREMENTAL
    And a one-time VACUUM compacts the database

  Scenario: Auto-vacuum check is skipped if already configured
    Given a database with auto_vacuum already set to INCREMENTAL
    When the startup guard chain runs
    Then no VACUUM operation runs

  # --- Schema compatibility ---

  Scenario: Database with known migrations passes compatibility check
    Given a database whose applied migrations are all known to this binary
    When the startup guard chain runs
    Then the compatibility check passes

  Scenario: Database with unknown migrations fails compatibility check
    Given a database with migrations not known to this binary
    When the startup guard chain runs
    Then startup fails with a schema incompatibility error
    And the error identifies which migrations are unknown
