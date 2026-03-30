@wip
Feature: Profile switch endpoint

  Any authenticated user can switch between demo and production profiles
  without a server restart. The switch is a session-only operation: the
  current session is invalidated, a new session is created for the same
  user in the target profile's database, and the active_profile file is
  updated to reflect the last-used profile.

  # --- Successful Switches ---

  Scenario: Authenticated user switches from demo to production
    Given the server has both demo and production databases open
    And I am logged in as the demo admin
    When I POST to "/api/profile/switch" with body {"profile": "production"}
    Then the response status is 200
    And the response body includes "profile" as "production"
    And my session now authenticates against the production database
    And the active_profile file contains "production"

  Scenario: Authenticated user switches from production to demo
    Given the server has both demo and production databases open
    And I am logged in as a production user
    When I POST to "/api/profile/switch" with body {"profile": "demo"}
    Then the response status is 200
    And the response body includes "profile" as "demo"
    And my session now authenticates as the demo admin
    And the active_profile file contains "demo"

  Scenario: Switching to the currently active profile is a no-op
    Given I am logged in as the demo admin
    When I POST to "/api/profile/switch" with body {"profile": "demo"}
    Then the response status is 200
    And the response body includes "profile" as "demo"

  # --- Authentication Guard ---

  Scenario: Unauthenticated request is rejected
    Given I am not logged in
    When I POST to "/api/profile/switch" with body {"profile": "production"}
    Then the response status is 401

  # --- Rate Limiting ---

  Scenario: Switching profiles three times in 15 minutes is allowed
    Given I am logged in as the demo admin
    When I switch profiles 3 times within 15 minutes
    Then all three responses have status 200

  Scenario: A fourth switch within 15 minutes is rate limited
    Given I am logged in as the demo admin
    And I have switched profiles 3 times in the last 15 minutes
    When I POST to "/api/profile/switch" with body {"profile": "demo"}
    Then the response status is 429

  Scenario: Rate limit resets after 15 minutes
    Given I have exhausted my profile switch rate limit
    When 15 minutes have elapsed
    And I POST to "/api/profile/switch" with body {"profile": "production"}
    Then the response status is 200

  # --- Origin Validation ---

  Scenario: Request with missing Origin header is rejected
    Given I am logged in as the demo admin
    When I POST to "/api/profile/switch" without an Origin header
    Then the response status is 400

  Scenario: Request with invalid Origin is rejected
    Given I am logged in as the demo admin
    When I POST to "/api/profile/switch" with Origin "http://evil.example.com"
    Then the response status is 400

  Scenario: Request from a valid same-origin host is accepted
    Given I am logged in as the demo admin
    When I POST to "/api/profile/switch" with a valid Origin header
    Then the response status is 200

  # --- Target User Lookup ---

  Scenario: Production switch looks up user by email in production database
    Given I am logged in as "owner@myshop.com" on production
    When I POST to "/api/profile/switch" with body {"profile": "demo"}
    And I switch back to production
    Then my authenticated email is "owner@myshop.com"

  Scenario: Demo switch always logs in as the demo admin user
    Given I am logged in as any production user
    When I POST to "/api/profile/switch" with body {"profile": "demo"}
    Then the authenticated user email is "admin@demo.local"

  Scenario: Switch to production fails if user has no account there
    Given I am logged in as the demo admin
    And no user exists in the production database
    When I POST to "/api/profile/switch" with body {"profile": "production"}
    Then the response status is 503
    And the response body includes a message about no account found

  # --- Session Integrity ---

  Scenario: Old session cookie is invalidated after switch
    Given I am logged in as the demo admin
    And I have a valid session cookie
    When I POST to "/api/profile/switch" with body {"profile": "production"}
    Then my old session cookie is no longer valid
    And a new session cookie is set

  Scenario: active_profile file is written atomically
    Given the server is running with both profiles available
    When a profile switch completes successfully
    Then the active_profile file contains only "demo" or "production"
    And no partial write state is observable
