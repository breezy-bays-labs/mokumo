Feature: Billing

  Scenario: User views invoice
    Given a user with an active subscription
    When the user opens the billing page
    Then the invoice is displayed
