Feature: Authentication

  Scenario: User logs in
    Given a user with email "test@example.com"
    When the user enters their password
    Then the user is logged in

  Scenario: User logs out
    Given a logged-in user
    When the user clicks logout
    Then the user is logged out
