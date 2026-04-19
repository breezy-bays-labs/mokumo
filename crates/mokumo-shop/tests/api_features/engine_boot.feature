@future
Feature: Engine Boot Integration

  The Mokumo server and desktop app boot through the kikan engine.
  The engine handles database preparation, migration, and startup
  while the binary handles error presentation.

  # --- Demo recovery ---

  Scenario: Demo database is recreated when incompatible
    Given a demo database with migrations from a newer version
    When the server starts
    Then the demo database is recreated from the sidecar
    And the server boots successfully with fresh demo data

  Scenario: Production database incompatibility fails with structured error
    Given a production database with migrations from a newer version
    When the server starts
    Then the server fails to start
    And the error identifies the unknown migrations

  # --- Headless error presentation ---

  Scenario: Headless binary exits with code 75 on boot failure
    Given a database that fails the startup guard chain
    When the headless server starts
    Then the process exits with code 75
    And structured error JSON is written to stderr

  # --- Desktop error presentation ---

  Scenario: Desktop app shows error dialog on boot failure
    Given a database that fails the startup guard chain
    When the desktop app starts
    Then an error event is emitted to the Tauri window
    And the error includes a machine-readable code and recovery actions
