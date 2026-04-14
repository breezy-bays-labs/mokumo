Feature: Demo installation guard

  When Mokumo boots in demo mode without a properly seeded database,
  the server enters a degraded state rather than crashing. Unauthenticated
  routes remain accessible so the problem is visible, but all protected
  routes are blocked until the demo data is restored via a demo reset.

  Note: supersedes the "Demo mode handles missing admin gracefully" scenario
  in demo_auth.feature — the new response is 423 DEMO_SETUP_REQUIRED rather
  than a generic error message.

  Background:
    Given the API server is running

  # --- Boot state ---

  Scenario: Health reports install_ok true when demo is properly seeded
    Given the server started with a correctly seeded demo database
    When I request GET "/api/health"
    Then the response status should be 200
    And the response should include "install_ok" with value true
    And the response should include "status" with value "ok"

  Scenario: Health reports install_ok false when demo admin is missing
    Given the server started with a demo database that has no admin account
    When I request GET "/api/health"
    Then the response status should be 200
    And the response should include "install_ok" with value false
    And the response should include "status" with value "degraded"

  # --- Unauthenticated routes stay accessible ---

  Scenario: Health endpoint remains accessible when installation is incomplete
    Given the server started with a demo database that has no admin account
    When I request GET "/api/health" without credentials
    Then the response status should be 200

  # --- Protected routes blocked ---

  Scenario: Protected routes return locked when installation is incomplete
    Given the server started with a demo database that has no admin account
    When I request GET "/api/customers" without credentials
    Then the response status should be 423
    And the response error code should be "DEMO_SETUP_REQUIRED"

  Scenario: Locked response carries a message and no details
    Given the server started with a demo database that has no admin account
    When I request GET "/api/customers" without credentials
    Then the response status should be 423
    And the json path "message" should not be empty
    And the json path "details" should be null

  # --- Reset clears the degraded state ---

  Scenario: Server re-evaluates installation after a demo reset
    Given the server started with a demo database that has no admin account
    And the demo sidecar contains a correctly seeded database
    When a client sends a demo reset request
    Then after the server restarts the health endpoint returns "install_ok" as true
