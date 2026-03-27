Feature: Migration Safety

  Before upgrading the database schema, Mokumo creates a backup
  so shop data is never lost during an update.

  Scenario: Database is backed up before schema upgrade
    Given an existing database at schema version 1
    When a schema upgrade to version 2 runs
    Then a backup file "mokumo.db.backup-v1" is created
    And the backup contains the original data

  Scenario: Only the last three backups are kept
    Given the database is at schema version 4
    And backups exist from previous upgrades to versions 2, 3, and 4
    When a schema upgrade to version 5 runs
    Then a backup of version 4 is created before upgrading
    And the oldest backup is removed
    And three backup files remain

  Scenario: No backup on first run
    Given no database file exists
    When the database is initialized for the first time
    Then no backup file is created

  # --- Migration atomicity ---

  Scenario: A failed schema upgrade leaves the database unchanged
    Given a database with all current migrations applied
    When a migration containing invalid SQL is applied
    Then the migration should fail
    And the database schema should be identical to before the attempt
    And no partial changes should be visible

  Scenario: Every migration runs inside a transaction
    Given the migration registry
    Then every registered migration should be marked as transactional

  # --- Cloud backup (future) ---

  @future
  Scenario: Database backup is uploaded to configured cloud storage
    Given the shop has configured cloud backup to S3
    When a schema upgrade runs
    Then a local backup is created before upgrading
    And the backup is uploaded to cloud storage

  @future
  Scenario: Shop owner can choose cloud backup destination
    Given the shop owner opens backup settings
    When they configure backup to Google Drive
    Then future schema upgrades include cloud backup
    And the shop owner can verify their last backup timestamp
