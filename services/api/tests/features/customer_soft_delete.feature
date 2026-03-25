Feature: Customer soft delete

  Customers are never permanently deleted. Deleting a customer
  sets a timestamp marking when it was removed, preserving the
  record for audit trails and potential restoration.

  Background:
    Given the API server is running

  # --- Soft delete behavior ---
  # Note: DELETE response shape (returns entity with deleted_at) is specified
  # in api_response_conventions.feature. This file focuses on filtering.

  Scenario: A deleted customer is not returned by default
    Given a customer exists
    And that customer has been deleted
    When I retrieve that customer by ID
    Then the response status should be 404

  Scenario: A deleted customer can be retrieved when including deleted
    Given a customer "Acme Corp" exists
    And that customer has been deleted
    When I retrieve that customer by ID including deleted
    Then the response status should be 200
    And the customer display name should be "Acme Corp"
    And the customer should have a "deleted_at" timestamp

  # --- List filtering ---

  Scenario: Deleted customers are excluded from lists by default
    Given a customer "Active Co" exists
    And a customer "Gone Co" exists
    And "Gone Co" has been deleted
    When I list customers
    Then the list should contain "Active Co"
    And the list should not contain "Gone Co"

  Scenario: Deleted customers are included when requested
    Given a customer "Active Co" exists
    And a customer "Gone Co" exists
    And "Gone Co" has been deleted
    When I list customers including deleted
    Then the list should contain both "Active Co" and "Gone Co"
    And "Gone Co" should have a "deleted_at" timestamp

  # --- Operations on deleted customers ---

  Scenario: Updating a deleted customer returns not found
    Given a customer exists
    And that customer has been deleted
    When I update that customer's display name to "New Name"
    Then the response status should be 404

  Scenario: Deleting an already-deleted customer returns not found
    Given a customer exists
    And that customer has been deleted
    When I delete that customer again
    Then the response status should be 404
