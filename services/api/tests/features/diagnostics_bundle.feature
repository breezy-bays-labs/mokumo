Feature: Diagnosis bundle export

  The bundle endpoint assembles a downloadable ZIP containing app logs and
  runtime metadata so that support staff can inspect a shop's state without
  SSH access.

  Background:
    Given the API server is running
    And an admin user is logged in

  Scenario: Authenticated user can download a diagnosis bundle
    When I request GET "/api/diagnostics/bundle"
    Then the response status should be 200
    And the response content type should contain "application/zip"
    And the response should have header "content-disposition" containing "attachment"
    And the response should have header "content-disposition" containing "mokumo-diagnostics"

  @wip
  Scenario: Bundle download requires authentication
    # Requires a fresh unauthenticated server — current test world shares session state.
    # Auth is enforced by the same require_auth_with_demo_auto_login middleware as
    # GET /api/diagnostics (already tested in the protected-route BDD scenarios).
    When I request GET "/api/diagnostics/bundle" without credentials
    Then the response status should be 401

  Scenario: Bundle response body is non-empty
    When I request GET "/api/diagnostics/bundle"
    Then the response body should not be empty
