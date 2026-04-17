Feature: Number Sequences

  Mokumo generates human-readable sequential identifiers for business
  entities — customer numbers (C-0001), quote numbers (Q-0001), invoice
  numbers (INV-0001). These are distinct from internal IDs and serve a
  business purpose: customers reference them on the phone, invoices
  need sequential numbering for accounting compliance.

  Scenario: First customer number is generated
    Given the customer sequence is seeded with prefix "C" and padding 4
    When the next customer number is requested
    Then the result is "C-0001"

  Scenario: Numbers increment sequentially
    Given the customer sequence is seeded with prefix "C" and padding 4
    And 3 customer numbers have already been generated
    When the next customer number is requested
    Then the result is "C-0004"

  Scenario: Numbers are zero-padded to the configured width
    Given a sequence with prefix "INV" and padding 6
    When the next number is requested
    Then the result is "INV-000001"

  Scenario: Numbers grow beyond the padding width
    Given the customer sequence is seeded with prefix "C" and padding 4
    And 9999 customer numbers have already been generated
    When the next customer number is requested
    Then the result is "C-10000"

  Scenario: Requesting a nonexistent sequence returns an error
    When the next number is requested for a sequence named "nonexistent"
    Then a "not found" error is returned

  Scenario: Different sequences increment independently
    Given the customer sequence is seeded with prefix "C" and padding 4
    And a quote sequence is seeded with prefix "Q" and padding 4
    And 5 customer numbers have already been generated
    When the next quote number is requested
    Then the result is "Q-0001"

  Scenario: Concurrent requests produce unique numbers
    Given the customer sequence is seeded with prefix "C" and padding 4
    When 10 customer numbers are requested simultaneously
    Then all 10 results are unique
    And the results are "C-0001" through "C-0010"
