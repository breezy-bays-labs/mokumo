@wip
Feature: Demo data reset

  Shop owners can reset the demo database to its original state
  from the Settings page. Reset only affects the demo database —
  production data is never resettable from within the app.

  Scenario: Reset replaces demo database with fresh sidecar
    Given the server is running in demo mode
    And the demo database has been modified
    When a client sends a demo reset request
    Then the demo database is replaced with a fresh copy of the sidecar

  Scenario: Reset triggers graceful server shutdown
    Given the server is running in demo mode
    When a client sends a demo reset request
    Then the server initiates a graceful shutdown
    And in-flight requests are allowed to complete

  Scenario: Reset completes without database corruption
    Given the server is running in demo mode
    And the demo database has active connections
    When a client sends a demo reset request
    Then the reset completes successfully
    And the demo database matches the original sidecar

  Scenario: Reset is rejected in production mode
    Given the server is running in production mode
    When a client sends a demo reset request
    Then the request is rejected with a forbidden status

  Scenario: Reset is rejected without authentication
    Given the server is running in demo mode
    When an unauthenticated client sends a demo reset request
    Then the request is rejected with an unauthorized status
