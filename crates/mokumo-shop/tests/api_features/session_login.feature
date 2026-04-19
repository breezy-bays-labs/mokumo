Feature: Session-Based Login

  Shop staff authenticate with email and password. Sessions are stored
  server-side with HTTP-only cookies. All application routes require
  an active session.

  Scenario: Valid credentials create a session
    Given an admin user exists
    When the user logs in with correct email and password
    Then the user is authenticated
    And the user remains authenticated for subsequent requests
    And the login attempt is recorded in the activity log

  Scenario: Invalid credentials are rejected
    Given an admin user exists
    When someone attempts to log in with an incorrect password
    Then no session is created
    And the failed attempt is recorded in the activity log

  Scenario: Unauthenticated requests are rejected
    Given the server is running with setup complete
    When an unauthenticated request hits a protected route
    Then the response is 401 Unauthorized

  Scenario: Valid session grants access to protected routes
    Given an admin user is logged in
    When the user requests a protected route
    Then the request succeeds with the user's identity

  Scenario: Expired session requires re-authentication
    Given an admin user has a session that has expired
    When the user requests a protected route
    Then the response is 401 Unauthorized

  Scenario: Logout destroys the session
    Given an admin user is logged in
    When the user logs out
    Then the user is no longer authenticated
    And subsequent requests require re-authentication
