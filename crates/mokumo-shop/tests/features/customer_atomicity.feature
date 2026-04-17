@future
Feature: Customer mutation activity-log atomicity

  Every customer mutation is logged to the platform activity log
  inside the same transaction as the mutation itself. A customer row
  and its activity entry either both land or neither does — there is
  no window where a customer exists without its audit trail, and no
  window where the audit trail references a customer that was never
  persisted.

  The vertical adapter is responsible for calling the platform
  ActivityWriter within its own transaction. The `kikan::ActivityWriter`
  trait gives the vertical access to the shared `activity_log` table
  without coupling to SeaORM-specific helpers.

  Action-string continuity: the literals written to `activity_log.action`
  are byte-identical to the pre-Stage-3 values (`"created"`,
  `"updated"`, `"soft_deleted"` — un-prefixed). The `entity_type`
  column already disambiguates between verticals (see R13).

  Background:
    Given a profile database with an empty customer table
    And an ActivityWriter test double that captures the
        &DatabaseTransaction pointer it receives on each call

  # --- Successful mutations share the mutation's transaction ---

  Scenario: Creating a customer calls the writer with the mutation's transaction
    When I create a customer "Acme Corp"
    Then a customer row exists with display name "Acme Corp"
    And an activity_log row exists with action "created" and entity_type "customer"
    And the ActivityWriter received the same &DatabaseTransaction pointer
        that was used for the customer INSERT

  Scenario: Updating a customer calls the writer with the mutation's transaction
    Given a customer "Acme Corp" exists
    When I update that customer's display name to "Acme Industries"
    Then the customer row reflects the new display name
    And a new activity_log row exists with action "updated" and entity_type "customer"
    And the ActivityWriter received the same &DatabaseTransaction pointer
        that was used for the customer UPDATE

  Scenario: Soft-deleting a customer calls the writer with the mutation's transaction
    Given a customer "Acme Corp" exists
    When I soft-delete that customer
    Then the customer row has a non-null deleted_at
    And a new activity_log row exists with action "soft_deleted" and entity_type "customer"
    And the ActivityWriter received the same &DatabaseTransaction pointer
        that was used for the customer UPDATE

  # --- Failure rollbacks ---

  Scenario: Customer insert rolls back when the activity write fails
    Given the ActivityWriter test double is configured to fail on the next call
    When I attempt to create a customer "Acme Corp"
    Then the create returns an activity-write error
    And no customer row exists with display name "Acme Corp"
    And the activity_log is empty

  Scenario: Aborting the commit after both inserts removes both rows
    Given a customer-create harness that aborts the commit after both inserts succeed
    When I attempt to create a customer "Acme Corp"
    Then no customer row exists with display name "Acme Corp"
    And the activity_log is empty

  # --- Actor propagation ---

  Scenario: The authenticated user is recorded as the actor
    Given an authenticated session for user "alice"
    When I create a customer "Acme Corp"
    Then the activity_log entry records actor_id equal to alice's user id
    And the activity_log entry records actor_type "user"

  Scenario: Unauthenticated mutation paths use the system actor
    Given the authenticated-session extractor yields no user
    When a mutation is attempted against the customer handler
    Then the handler returns an authentication error
    And the activity_log is empty

  # --- Payload shape ---

  Scenario: The activity payload captures the customer snapshot
    Given a customer "Acme Corp" exists
    When I update that customer's display name to "Acme Industries"
    Then the activity payload is a JSON snapshot of the customer row
        after the update
    And the payload's display_name field equals "Acme Industries"
    And the payload's id field equals the customer's UUID
