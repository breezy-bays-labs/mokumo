@future
Feature: User repository composite method atomicity (Pattern C)

  Multi-entity admin mutations are single composite UserRepo methods that
  open a transaction internally. Control plane handlers never see a
  DatabaseTransaction. Either every entity in the composite persists
  together, or none do — including the activity log entry.

  # This enforces CLAUDE.md #12 (activity-log-in-txn) at the structural
  # level for the control plane. Pattern C lives in
  # adr-activity-log-transaction-atomicity; this feature pins the contract.

  # --- Atomic success (create_user_with_codes) ---

  Scenario: Creating a user with recovery codes persists user, codes, and activity entry together
    Given an empty user table
    When the repo creates a user "alice@shop.example" with 10 recovery codes
    Then the user "alice@shop.example" exists
    And 10 recovery codes belong to that user
    And the activity log contains a "user.created" entry for that user

  # --- Atomic rollback on invalid recovery code batch ---

  Scenario: Create-user-with-codes rolls back when the recovery code batch is rejected
    Given an empty user table
    When the repo is asked to create "alice@shop.example" with a recovery code batch that fails validation
    Then the operation fails
    And no user "alice@shop.example" exists
    And no recovery codes exist
    And no activity log entry for user creation exists

  # --- Atomic success (regenerate_codes_with_log) ---

  Scenario: Regenerating recovery codes replaces the old batch and logs activity together
    Given a user "alice@shop.example" with 10 recovery codes
    When the repo regenerates that user's recovery codes
    Then the user has exactly 10 recovery codes
    And none of the new codes match the previous batch
    And the activity log contains a "recovery_codes.regenerated" entry for that user

  # --- Atomic rollback (regenerate_codes_with_log) ---

  Scenario: Regenerate rolls back when activity log insert fails
    Given a user "alice@shop.example" with 10 recovery codes
    And the activity log write is forced to fail
    When the repo regenerates that user's recovery codes
    Then the operation fails
    And the user still has exactly the original 10 recovery codes
    And no new activity log entry exists

  # --- Atomic success (bootstrap_admin_with_codes) ---

  Scenario: Bootstrap admin succeeds on an empty user table
    Given an empty user table
    When the repo bootstraps an admin "founder@shop.example" with 10 recovery codes
    Then the user "founder@shop.example" exists with role "admin"
    And 10 recovery codes belong to that user
    And the activity log contains a "user.bootstrap" entry

  # --- Bootstrap rejection when an admin already exists ---

  Scenario: Bootstrap admin is rejected when any admin already exists
    Given a user "existing@shop.example" with role "admin"
    When the repo attempts to bootstrap an admin "another@shop.example"
    Then the operation fails with code "ALREADY_BOOTSTRAPPED"
    And no user "another@shop.example" exists
    And no activity log entry for bootstrap exists

  # Structural invariant (UserRepo trait contains no sea_orm::DatabaseTransaction
  # or sea_orm::TransactionTrait in any signature) is enforced by a standalone
  # unit test in crates/kikan/tests/user_repo_trait_signature.rs. It is not a
  # runtime behavior and therefore not expressed as a BDD scenario.

  # Covered by unit tests: composite methods call TransactionTrait::transaction() internally.
