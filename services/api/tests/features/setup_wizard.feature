Feature: Setup Wizard

  A fresh Mokumo binary guides the shop owner through first-time setup:
  create the shop, set up the admin account, and display recovery codes.
  After setup completes, the wizard is permanently locked.

  Scenario: Fresh binary redirects to setup wizard
    Given a freshly started server with no users
    When an unauthenticated request hits a protected route
    Then the response indicates setup is incomplete

  Scenario: Setup wizard creates shop and admin account
    Given a freshly started server with no users
    And a valid setup token
    When the shop owner submits the setup wizard with shop name, admin credentials, and token
    Then a shop is created with the given name
    And an admin user is created with the given credentials
    And the admin account is secured
    And setup is marked as complete

  Scenario: Setup wizard returns recovery codes
    Given a freshly started server with no users
    And a valid setup token
    When the shop owner completes the setup wizard
    Then 10 recovery codes are returned in the response
    And the codes are securely stored for future verification

  Scenario: Admin is automatically logged in after setup
    Given a freshly started server with no users
    And a valid setup token
    When the shop owner completes the setup wizard
    Then a session is created for the new admin
    And the response includes a session cookie

  Scenario: Setup wizard is locked after initial setup
    Given the setup wizard has already been completed
    When someone attempts to access the setup wizard
    Then the request is rejected with a forbidden status

  Scenario: Concurrent setup attempts are rejected
    Given a freshly started server with no users
    And the first setup request is being processed
    When a second setup request arrives simultaneously
    Then only one admin account is created
    And the second request is rejected

  Scenario: Setup wizard rejects incomplete credentials
    Given a freshly started server with no users
    And a valid setup token
    When the shop owner submits the setup wizard with missing required fields
    Then the request is rejected with a validation error
    And no user account is created

  Scenario: Shop owner must confirm recovery codes before completing setup
    Given the setup wizard has returned recovery codes
    When the shop owner confirms they have saved a code
    Then the setup wizard allows proceeding to the final step

  Scenario: Invalid setup token is rejected
    Given a freshly started server with no users
    When someone submits the setup wizard with an incorrect token
    Then the request is rejected
    And no user account is created

  Scenario: Setup wizard completion clears is_first_launch without a prior profile switch
    Given a freshly started server with no users
    When the shop owner completes the setup wizard via the HTTP API
    Then GET /api/setup-status returns is_first_launch as false
