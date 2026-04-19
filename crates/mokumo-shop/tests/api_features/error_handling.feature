@wip
Feature: Standalone Startup Error Handling

  When something goes wrong during startup in standalone mode,
  Mokumo tells the developer what happened in plain language.
  Desktop error handling is specified in desktop-shell.feature.

  Scenario: Clear message when data directory cannot be created
    Given the data directory path is not writable
    When the server starts
    Then it exits with a message explaining the permission problem
    And the message includes the directory path

  Scenario: Migration failure in standalone mode
    Given the database migration fails
    When running as a standalone server
    Then it exits with a message explaining what went wrong

  Scenario: Server shuts down gracefully
    Given the server is running
    When a shutdown signal is received
    Then in-flight requests complete
    And database connections are closed
    And the process exits cleanly
