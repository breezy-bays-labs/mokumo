@wip
Feature: Demo database startup

  Mokumo ships a pre-seeded demo database so new users see a populated
  print shop on first launch. The server detects the data directory
  layout, copies the demo sidecar if needed, and resolves which
  database profile to use.

  # --- New User (First Launch) ---

  Scenario: First launch creates demo directory structure
    Given a fresh data directory with no databases
    When the server starts
    Then a "demo" subdirectory exists
    And a "production" subdirectory exists

  Scenario: First launch copies demo sidecar to demo directory
    Given a fresh data directory with no databases
    And a demo.db sidecar is available
    When the server starts
    Then "demo/mokumo.db" exists in the data directory
    And the active profile is "demo"

  Scenario: First launch opens the demo database
    Given a fresh data directory with no databases
    And a demo.db sidecar is available
    When the server starts
    Then the server is connected to the demo database
    And the health endpoint returns healthy

  Scenario: Demo database has pre-seeded customers
    Given the server started with a demo sidecar
    When a client requests the customer list
    Then at least 25 customers are returned

  Scenario: First launch without sidecar starts in fresh mode
    Given a fresh data directory with no databases
    And no demo.db sidecar is available
    When the server starts
    Then the server starts successfully
    And setup is not complete
    And the active profile defaults to fresh install behavior

  Scenario: Demo database has activity history
    Given the server started with a demo sidecar
    When a client requests the activity log for a customer
    Then the activity log contains at least one entry

  # --- Existing User (Upgrade Migration) ---

  Scenario: Existing flat layout is migrated to dual-directory structure
    Given a data directory with a flat "mokumo.db" file
    When the server starts
    Then "production/mokumo.db" exists in the data directory
    And the active profile is "production"
    And the original flat "mokumo.db" is removed

  Scenario: Migration preserves existing data
    Given a data directory with a flat "mokumo.db" containing 5 customers
    When the server starts
    Then the server is connected to the production database
    And the customer list contains exactly 5 customers

  Scenario: Migration copies demo sidecar alongside production
    Given a data directory with a flat "mokumo.db" file
    And a demo.db sidecar is available
    When the server starts
    Then "demo/mokumo.db" exists in the data directory
    And "production/mokumo.db" exists in the data directory

  Scenario: Migration is idempotent after crash
    Given a data directory with both "production/mokumo.db" and flat "mokumo.db"
    When the server starts
    Then the server starts successfully
    And "production/mokumo.db" contains the same number of customers as before
    And the flat "mokumo.db" is removed

  # --- Session Store Separation ---

  Scenario: Sessions are stored in a separate database
    Given the server is running
    Then "sessions.db" exists in the root data directory
    And "sessions.db" is not inside the demo or production subdirectory

  Scenario: Session store is independent of the active profile database
    Given the server started with the demo profile
    When a session is created
    Then the session is stored in "sessions.db"
    And the demo database does not contain session tables

  # --- Startup Migrations ---

  Scenario: Startup runs migrations on the demo database
    Given a demo database with an older schema version
    When the server starts
    Then the demo database schema is up to date

  Scenario: Startup runs migrations on the production database if it exists
    Given a production database with an older schema version
    When the server starts
    Then the production database schema is up to date
