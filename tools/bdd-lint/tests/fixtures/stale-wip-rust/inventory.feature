Feature: Inventory Management

  @wip
  Scenario: Add item to inventory
    Given an empty warehouse
    When an item "Widget" is added with quantity 10
    Then the inventory should contain "Widget"

  Scenario: Check stock levels
    Given an empty warehouse
    When an item "Gadget" is added with quantity 5
    Then the inventory should contain "Gadget"
