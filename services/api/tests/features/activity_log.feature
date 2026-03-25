Feature: Activity log

  Every mutation to a customer is recorded in an append-only
  activity log. Each entry captures what changed, who made the
  change, and a full snapshot of the entity at that moment.

  Background:
    Given the API server is running

  # --- Automatic logging ---

  Scenario: Creating a customer logs a "created" activity
    When I create a customer "Acme Corp"
    Then the activity log for that customer should have 1 entry
    And the latest activity action should be "created"
    And the activity actor should be "system"
    And the activity payload should contain the customer snapshot

  Scenario: Updating a customer logs an "updated" activity
    Given a customer "Acme Corp" exists
    When I update that customer's display name to "Acme Industries"
    Then the latest activity action for that customer should be "updated"
    And the activity payload should reflect the updated name

  Scenario: Deleting a customer logs a "soft_deleted" activity
    Given a customer exists
    When I delete that customer
    Then the latest activity action for that customer should be "soft_deleted"

  # --- Query endpoint ---

  Scenario: Querying activity for a specific customer
    Given a customer "Acme Corp" exists
    And that customer has been updated twice
    When I query activity for that customer
    Then I should see 3 activity entries
    And the entries should be in newest-first order

  Scenario: Querying activity by entity type
    Given a customer "Alpha" exists
    And a customer "Beta" exists
    When I query activity for entity type "customer"
    Then I should see activity entries for both customers

  Scenario: Querying activity with pagination
    Given a customer exists with 30 activity entries
    When I query activity for that customer with 10 per page
    Then the response should contain 10 items
    And the total should be 30

  # --- Append-only guarantee ---

  Scenario: Activity entries cannot be modified or deleted
    Given a customer has activity entries
    Then there is no endpoint to update activity entries
    And there is no endpoint to delete activity entries

  # --- Empty state ---

  Scenario: Querying activity for a customer with no changes
    When I query activity for a non-existent entity
    Then the response should contain 0 items
    And the total should be 0
