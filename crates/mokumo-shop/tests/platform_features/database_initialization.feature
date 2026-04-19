Feature: Database Initialization

  Mokumo automatically creates and configures its database on first
  run. The shop owner never interacts with the database directly.

  # PRAGMA verification (WAL, foreign_keys, busy_timeout, synchronous,
  # cache_size) belongs in platform unit tests, not in this behavioral
  # specification. See tests/platform_database_init.rs.

  @allow.skipped
  Scenario: Database is created automatically on first run
    Given no database file exists
    When the server starts for the first time
    Then a database file is created in the data directory
    And the database is ready to accept data

  @allow.skipped
  Scenario: Database initialization is idempotent
    Given the database has already been initialized
    When the server starts again
    Then no error occurs
    And existing data is preserved
