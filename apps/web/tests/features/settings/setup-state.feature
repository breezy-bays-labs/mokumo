Feature: Setup-State Indicator in System Settings

  System Settings shows a mode indicator so admins can confirm which profile
  (demo or production) the server is currently running.

  Scenario: Production mode indicator is visible in System Settings
    Given the system is in production mode
    When I navigate to the System Settings page
    Then I see the "Production Mode" section
    And I see an "Active" badge next to "Production Mode"

  Scenario: Demo mode does not show the production section
    Given the system is in demo mode
    When I navigate to the System Settings page
    Then I do not see the "Production Mode" section
    And I see the "Demo Mode" section

  Scenario: After switching from demo to production, System Settings reflects the new mode
    Given the system is in demo mode
    When I navigate to the System Settings page
    And the profile is switched to production mode
    And I navigate to the System Settings page
    Then I see the "Production Mode" section
    And I do not see the "Demo Mode" section
