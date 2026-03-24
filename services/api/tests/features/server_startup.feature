@wip
Feature: Server Startup

  The Mokumo server starts with sensible defaults so a shop owner
  never needs to configure anything. Developers can override defaults
  with CLI flags.

  Scenario: Server starts with no configuration
    Given no CLI flags are provided
    When the server starts
    Then it listens on port 6565
    And it uses the platform default data directory
    And the data directory is created automatically

  Scenario: Server creates required subdirectories
    Given a fresh data directory
    When the server starts
    Then a "logs" subdirectory exists
    And a "backups" subdirectory exists

  Scenario: CLI flag overrides the default port
    Given the flag "--port 7000" is provided
    When the server starts
    Then it listens on port 7000

  Scenario: CLI flag overrides the default host
    Given the flag "--host 127.0.0.1" is provided
    When the server starts
    Then it listens on host 127.0.0.1

  Scenario: CLI flag overrides the data directory
    Given the flag "--data-dir /tmp/mokumo-test" is provided
    When the server starts
    Then the data directory is "/tmp/mokumo-test"

  Scenario: Server serves the web application
    Given the server is running
    When a browser requests the root path
    Then the SvelteKit application is returned

  Scenario: Health endpoint reports server status
    Given the server is running
    When a client requests the health endpoint
    Then it receives a healthy status
    And the response includes the server version
