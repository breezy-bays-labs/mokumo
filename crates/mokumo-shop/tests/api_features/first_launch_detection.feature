@wip
Feature: First-launch detection via setup-status

  On a fresh install, the active_profile file is absent. The server reads
  this at startup and exposes is_first_launch: true on the setup-status
  endpoint. Once any profile switch completes, the file is written and
  subsequent startups report is_first_launch: false.

  # --- is_first_launch field ---

  Scenario: Fresh install reports is_first_launch as true
    Given the server starts with no active_profile file present
    When a client requests GET /api/setup-status
    Then the response includes "is_first_launch" as true

  Scenario: After a profile switch, is_first_launch is false
    Given the server starts with no active_profile file present
    And an authenticated user switches to the demo profile
    When a client requests GET /api/setup-status
    Then the response includes "is_first_launch" as false

  Scenario: Server restart after first switch still reports is_first_launch as false
    Given a profile switch has previously occurred and active_profile file exists
    When the server restarts and a client requests GET /api/setup-status
    Then the response includes "is_first_launch" as false

  Scenario: is_first_launch does not change during a running session
    Given the server started with is_first_launch as true
    When a profile switch writes the active_profile file
    Then the in-memory is_first_launch AtomicBool is updated to false
    And subsequent setup-status requests return is_first_launch as false

  @wip
  Scenario: Setup wizard completion clears is_first_launch
    Given the server started with is_first_launch as true
    When I POST to "/api/setup/complete" with valid shop configuration
    And a client requests GET /api/setup-status
    Then the response includes "is_first_launch" as false

  # --- production_setup_complete field ---

  Scenario: Setup status includes production_setup_complete as false before setup
    Given no production setup has been completed
    When a client requests GET /api/setup-status
    Then the response includes "production_setup_complete" as false

  Scenario: Setup status includes production_setup_complete as true after setup
    Given the production setup wizard has been completed
    When a client requests GET /api/setup-status
    Then the response includes "production_setup_complete" as true

  # --- shop_name field ---

  Scenario: Setup status includes shop_name as null before production setup
    Given no production setup has been completed
    When a client requests GET /api/setup-status
    Then the response includes "shop_name" as null

  Scenario: Setup status includes shop_name after production setup
    Given the production setup wizard completed with shop name "Gary's Printing Co"
    When a client requests GET /api/setup-status
    Then the response includes "shop_name" as "Gary's Printing Co"
