@wip
Feature: Port Fallback

  When the preferred port is occupied, Mokumo tries the next
  available port so the shop owner never sees a cryptic bind error.

  Scenario: Server uses the next port when default is occupied
    Given port 6565 is already in use
    When the server starts
    Then it listens on port 6566
    And the actual port is logged

  Scenario: Server tries up to 10 ports
    Given ports 6565 through 6574 are already in use
    When the server starts
    Then it listens on port 6575

  Scenario: Server fails when all fallback ports are occupied
    Given ports 6565 through 6575 are already in use
    When the server starts
    Then it exits with a clear port error message
