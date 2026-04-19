Feature: Health endpoint storage status

  The health endpoint reports a lean storage_ok flag so that monitoring
  tools and the Tauri shell can detect disk-pressure or fragmentation
  without authenticating. The underlying detail is available only via
  the authenticated diagnostics endpoint.

  Background:
    Given the API server is running

  # --- Normal operation ---


  Scenario: Health reports storage_ok true under normal conditions
    Given the active database is not fragmented
    And disk space is above the warning threshold
    When I request GET "/api/health"
    Then the response status should be 200
    And the response should include "storage_ok" with value true
    And the response should include "status" with value "ok"

  # --- Degraded paths ---


  Scenario: Health reports storage_ok false when active database is fragmented
    Given the active database is heavily fragmented
    And disk space is above the warning threshold
    When I request GET "/api/health"
    Then the response should include "storage_ok" with value false
    And the response should include "status" with value "degraded"


  Scenario: Health reports storage_ok false when disk space is low
    Given the active database is not fragmented
    And disk space is below the warning threshold
    When I request GET "/api/health"
    Then the response should include "storage_ok" with value false
    And the response should include "status" with value "degraded"


  Scenario: storage_ok reflects the active profile not the inactive one
    Given the inactive database is heavily fragmented
    And the active database is not fragmented
    And disk space is above the warning threshold
    When I request GET "/api/health"
    Then the response should include "storage_ok" with value true

  # --- Combined status truth table ---


  Scenario: Status is ok when both install and storage checks pass
    Given the server started with a correctly seeded demo database
    And the active database is not fragmented
    And disk space is above the warning threshold
    When I request GET "/api/health"
    Then the response should include "status" with value "ok"
    And the response should include "install_ok" with value true
    And the response should include "storage_ok" with value true


  Scenario: Status is degraded when install check fails regardless of storage
    Given the server started with a demo database that has no admin account
    And disk space is above the warning threshold
    When I request GET "/api/health"
    Then the response should include "status" with value "degraded"

  # --- Public access ---


  Scenario: Storage status is accessible without authentication
    When I request GET "/api/health" without credentials
    Then the response status should be 200
    And the response should include "storage_ok"
