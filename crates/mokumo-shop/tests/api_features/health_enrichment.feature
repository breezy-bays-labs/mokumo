Feature: Health endpoint enrichment

  The health endpoint reports server status, uptime, and database
  connectivity so that monitoring tools and the Tauri shell can
  verify the system is operational.

  Background:
    Given the API server is running

  # --- Enriched response shape ---

  Scenario: Health check returns uptime and database status
    When I request GET "/api/health"
    Then the response status should be 200
    And the response should include "status" with value "ok"
    And the response should include "version"
    And the response should include "uptime_seconds" as a non-negative integer
    And the response should include "database" with value "ok"

  # --- Uptime tracking ---

  Scenario: Uptime increases over time
    Given I have recorded the uptime from a health check
    When I request GET "/api/health" after a brief delay
    Then the uptime should be greater than or equal to the previous value

  # --- Cache control ---

  Scenario: Health responses are not cached
    When I request GET "/api/health"
    Then the response should have header "Cache-Control" with value "no-store"

  # --- Public access ---

  Scenario: Health endpoint is accessible without authentication
    When I request GET "/api/health" without credentials
    Then the response status should be 200

  # --- Database failure ---

  Scenario: Health check fails when the database is unreachable
    Given the database is unavailable
    When I request GET "/api/health"
    Then the response status should be 500
