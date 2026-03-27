Feature: Authentication

  Scenario: User views profile
    Given the user is authenticated
    When the user opens the profile page
    Then the profile is displayed
