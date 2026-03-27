Feature: Checkout

  Scenario: User completes checkout
    Given a cart with 3 items
    When the user enters their shipping address
    Then the order is placed

  Scenario: User applies coupon
    Given a cart with items
    When the user applies coupon "SAVE10"
    Then the total is reduced by 10 percent
