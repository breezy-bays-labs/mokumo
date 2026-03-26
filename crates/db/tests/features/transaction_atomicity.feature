Feature: Mutation and activity log atomicity

  Every customer mutation (create, update, soft-delete) and its
  corresponding activity log entry are persisted together as a
  single atomic operation. Either both succeed or neither persists.
  This prevents orphaned records — a customer without an audit trail,
  or an audit entry for a mutation that never committed.

  # --- Atomic success ---

  Scenario: Creating a customer persists both the record and its activity entry
    Given an empty database
    When a customer "Acme Corp" is created
    Then the customer "Acme Corp" should exist
    And the activity log should contain a "created" entry for that customer
    And the activity entry should record the customer's details at creation

  Scenario: Updating a customer persists both the change and its activity entry
    Given a customer "Acme Corp" exists in the database
    When that customer's display name is changed to "Acme Industries"
    Then the customer's display name should be "Acme Industries"
    And the activity log should contain an "updated" entry for that customer

  Scenario: Soft-deleting a customer persists both the deletion and its activity entry
    Given a customer "Acme Corp" exists in the database
    When that customer is soft-deleted
    Then the customer should be marked as deleted
    And the activity log should contain a "soft_deleted" entry for that customer

  # --- Atomic rollback ---

  Scenario: A failed mutation does not leave an orphaned activity entry
    Given a customer "Acme Corp" exists in the database
    And that customer has 1 activity entry
    When an update to a non-existent customer is attempted
    Then the operation should have failed
    And the activity log for "Acme Corp" should still have 1 entry
    And no new activity entries should exist

  # Covered by unit test: repo_create_rolls_back_on_activity_failure
  # True fault-injection atomicity (fail after INSERT, verify rollback)
  # is tested at the unit level, not BDD — see crates/db/src/customer/repo.rs

  # --- Activity entries record correct actions ---

  # Tests audit trail ordering across a full lifecycle — compound When is intentional
  Scenario: Activity entries use the correct entity type and action
    Given an empty database
    When a customer is created, then updated, then soft-deleted
    Then the activity log should contain 3 entries for that customer
    And the actions should be "created", "updated", "soft_deleted" in order

  # --- Read independence ---

  Scenario: Activity log reads work independently of mutation transactions
    Given a customer "Acme Corp" exists with 1 activity entry
    When the activity log is queried for entity type "customer"
    Then the response should contain 1 entry for "Acme Corp"
