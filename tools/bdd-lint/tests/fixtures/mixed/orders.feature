Feature: Orders

  Scenario: User places order
    Given a cart with items
    When the user places the order
    Then an order confirmation is shown

  Scenario: User tracks order
    Given an existing order
    When the user views tracking info
    Then the tracking status is shown
