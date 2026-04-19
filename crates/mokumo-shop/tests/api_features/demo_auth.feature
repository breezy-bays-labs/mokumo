Feature: Demo mode authentication

  When running in demo mode, Mokumo automatically logs in the
  pre-seeded admin so the user experiences the app without friction.
  The setup-status API reports the current mode.

  # --- Auto-Login ---

  Scenario: Demo mode auto-logs in the pre-seeded admin
    Given the server is running in demo mode
    When an unauthenticated request hits a protected route
    Then a session is automatically created for the demo admin
    And the response includes a session cookie

  Scenario: Demo admin identity matches the pre-seeded credentials
    Given the server is running in demo mode
    When the auto-login creates a session
    Then the authenticated user email is "admin@demo.local"
    And the authenticated user name is "Demo Admin"

  Scenario: Production mode does not auto-login
    Given the server is running in production mode
    When an unauthenticated request hits a protected route
    Then no automatic session is created
    And the response indicates authentication is required

  Scenario: Demo mode handles missing admin gracefully
    Given the server is running in demo mode
    And the demo database has no admin user
    When an unauthenticated request hits a protected route
    Then the response indicates an error with a helpful message

  # --- Setup Status API ---

  Scenario: Setup status reports demo mode
    Given the server is running in demo mode
    When a client requests the setup status
    Then the response includes "setup_complete" as true
    And the response includes "setup_mode" as "demo"

  Scenario: Setup status reports production mode
    Given the server is running in production mode with setup complete
    When a client requests the setup status
    Then the response includes "setup_complete" as true
    And the response includes "setup_mode" as "production"

  Scenario: Setup status reports fresh install
    Given the server is running with no setup completed
    When a client requests the setup status
    Then the response includes "setup_complete" as false
    And the response includes "setup_mode" as null

  # --- Setup Mode Caching ---

  Scenario: Setup mode is available immediately after startup
    Given a demo database with setup_mode set to "demo"
    When the server starts
    Then the setup-status response returns setup_mode "demo"
