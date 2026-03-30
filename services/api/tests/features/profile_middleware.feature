@wip
Feature: ProfileDbMiddleware injects correct database per session

  The ProfileDbMiddleware runs after AuthManagerLayer and injects a
  DatabaseConnection into request extensions via the ProfileDb extractor.
  For authenticated requests, it selects the database matching the user's
  profile discriminant (stored in the compound AuthUser::Id). For
  unauthenticated requests, it falls back to AppState.active_profile.

  # --- Authenticated routing ---

  Scenario: Authenticated demo user gets demo database
    Given a running server with both demo and production databases initialised
    And a demo user is logged in
    When the demo user calls a protected endpoint
    Then the request is served from the demo database

  Scenario: Authenticated production user gets production database
    Given a running server with both demo and production databases initialised
    And a production user is logged in
    When the production user calls a protected endpoint
    Then the request is served from the production database

  # --- Unauthenticated fallback ---

  Scenario: Unauthenticated request in demo mode uses demo database
    Given a running server with active profile set to demo
    When an unauthenticated request reaches a protected endpoint
    Then the request is rejected with 401 Unauthorized

  Scenario: Unauthenticated request in production mode uses production database
    Given a running server with active profile set to production
    When an unauthenticated request reaches a protected endpoint
    Then the request is rejected with 401 Unauthorized

  # --- Hardcoded routes bypass ProfileDb ---

  Scenario: demo_reset always uses demo database regardless of active profile
    Given a running server with active profile set to production
    And a demo user is logged in
    When the demo user calls POST /api/demo/reset
    Then the request is rejected with 403 Forbidden

  Scenario: setup endpoint uses production database
    Given a running server where setup is not yet complete
    When the setup endpoint is called with valid credentials
    Then the setup is stored in the production database
