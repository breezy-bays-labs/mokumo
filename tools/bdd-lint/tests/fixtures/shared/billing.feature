Feature: Billing

  Scenario: User views billing
    Given the user is authenticated
    When the user opens the billing page
    Then the billing summary is displayed

  @wip
  Scenario: User triggers maintenance check
    Given the system is under maintenance
    When the user opens the billing page
    Then a maintenance message is shown
