Feature: API response conventions

  Standard patterns that every API endpoint follows.
  These conventions ensure the frontend can handle
  all responses with a single fetch utility.

  Background:
    Given the API server is running

  # --- Bare T for success ---

  Scenario: Single item responses return the entity directly
    Given a customer exists
    When I request that customer
    Then the response body is the customer object itself

  Scenario: Creating a resource returns the new entity
    When I create a valid customer
    Then the response status should be 201
    And the response body is the created customer

  Scenario: Updating a resource returns the updated entity
    Given a customer exists
    When I update that customer's name
    Then the response status should be 200
    And the response body is the updated customer

  # --- Delete returns entity ---

  Scenario: Deleting a resource returns it with its deleted state
    Given a customer exists
    When I delete that customer
    Then the response status should be 200
    And the response body includes a "deleted_at" timestamp
    And the customer name is still present in the response

  # --- Soft-delete filtering ---

  Scenario: Deleted items are excluded from lists by default
    Given a customer exists
    And that customer has been deleted
    When I list customers
    Then the deleted customer should not appear in the results

  Scenario: Deleted items can be included when requested
    Given a customer exists
    And that customer has been deleted
    When I list customers including deleted
    Then the deleted customer should appear in the results
    And the deleted customer should have a "deleted_at" timestamp

  # --- Unmatched API routes ---

  Scenario: Requesting a non-existent API route returns a structured error
    When I request an API path that does not exist
    Then the response status should be 404
    And the error code should be "not_found"
