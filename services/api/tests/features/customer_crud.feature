Feature: Customer management

  Shop owners manage their customer records. Customers can be
  created, viewed, listed, updated, and soft-deleted.

  Background:
    Given the API server is running

  # --- Create ---

  Scenario: Creating a customer with required fields
    When I create a customer with display name "Acme Corp"
    Then the response status should be 201
    And the customer should have a UUID identifier
    And the customer display name should be "Acme Corp"

  Scenario: Creating a customer with all fields
    When I create a customer with full details
    Then the response status should be 201
    And the customer should have all provided fields populated

  # --- Get by ID ---

  Scenario: Retrieving a customer by ID
    Given a customer "Acme Corp" exists
    When I retrieve that customer by ID
    Then the response status should be 200
    And the customer display name should be "Acme Corp"

  Scenario: Retrieving a non-existent customer
    When I retrieve a customer with a random UUID
    Then the response status should be 404
    And the error code should be "not_found"

  # --- List ---

  Scenario: Listing customers returns paginated results
    Given 3 customers exist
    When I list customers
    Then the response should contain 3 items
    And the total should be 3

  # --- Update ---

  Scenario: Updating a customer's details
    Given a customer "Acme Corp" exists
    When I update that customer's display name to "Acme Industries"
    Then the response status should be 200
    And the customer display name should be "Acme Industries"

  Scenario: Updated customer has a newer updated_at timestamp
    Given a customer exists
    When I update that customer
    Then the customer's updated_at should be later than its created_at

  # --- Nullable field clearing ---

  Scenario: Setting a nullable field to null clears it
    Given a customer "Acme Corp" exists with email "old@acme.com"
    When I update that customer with email set to null
    Then the response status should be 200
    And the customer email should be null

  Scenario: Omitting a field from an update preserves its value
    Given a customer "Acme Corp" exists with email "keep@acme.com"
    When I update that customer's display name to "Acme Industries"
    Then the response status should be 200
    And the customer email should be "keep@acme.com"
