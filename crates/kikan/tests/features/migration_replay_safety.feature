@future
Feature: Migration replay safety across graft restructuring

  When a graft's migration files are physically relocated (for
  example, moving vertical migrations out of a platform crate into
  a vertical crate) the kikan engine must not replay any migration
  that was already applied. Migration identity is carried by the
  stored (graft_id, migration_name) pair — not by the source file's
  location on disk.

  This feature protects against the Stage 3 hazard: migration files
  move between crates, but the history table must remain
  authoritative and untouched.

  The pre-Stage-3 history under graft id "mokumo" names eight
  migrations (see the Examples table below). These names are the
  continuity anchor: any deviation causes a replay attempt.

  # --- Identity is (graft_id, migration_name) ---

  Scenario: A migration relocated to another crate is treated as already applied
    Given a profile database whose history records migration "m20260321_000000_init" under graft "mokumo"
    And a graft that declares "m20260321_000000_init" from a new source location
    When migrations are executed for that graft
    Then no migrations run
    And the history table is unchanged

  Scenario: A renamed migration runs as a new (unapplied) migration
    Given a profile database whose history records migration "m20260321_000000_init" under graft "mokumo"
    And a graft that declares "m20260321_000000_init_v2" instead
    When migrations are executed for that graft
    Then "m20260321_000000_init_v2" executes and is appended to the history
    And the existing row for "m20260321_000000_init" remains unchanged
    And no automatic reconciliation occurs between the two names

  Scenario: Changing the graft id does not reconcile prior history
    Given a profile database whose history records migrations under graft "mokumo"
    And a graft that now identifies itself as "garment"
    When migrations are executed for the "garment" graft
    Then the engine treats those migrations as unapplied for "garment"
    And the existing "mokumo" history rows are left in place
    And no rows are relabelled from "mokumo" to "garment"

  # --- Pre-Stage-3 snapshot fixture ---

  Scenario: Booting against a pre-Stage-3 profile database runs zero migrations
    Given the snapshot fixture captured before Stage 3 merged
    When the engine boots with the post-Stage-3 MokumoApp graft
    Then zero migrations execute
    And the kikan_migrations history has the same row count it had before boot
    And every row's (graft_id, name, applied_at) triple is unchanged

  Scenario: Booting against a pre-Stage-3 profile database preserves user data
    Given the snapshot fixture captured before Stage 3 merged
    When the engine boots with the post-Stage-3 MokumoApp graft
    Then every customer row from the snapshot is still queryable
    And every activity_log row from the snapshot is still queryable
    And every number-sequence row from the snapshot is still queryable

  # --- Pre-Stage-3 migration names are declared identically ---

  Scenario Outline: Each pre-Stage-3 migration name is declared by MokumoApp
    Given the post-Stage-3 MokumoApp graft
    When its migrations() are requested
    Then the returned list contains a migration named "<name>"
    And that migration's graft_id equals kikan::GraftId::new("mokumo")

    Examples:
      | name                                           |
      | m20260321_000000_init                          |
      | m20260322_000000_settings                      |
      | m20260324_000000_number_sequences              |
      | m20260324_000001_customers_and_activity        |
      | m20260326_000000_customers_deleted_at_index    |
      | m20260327_000000_users_and_roles               |
      | m20260404_000000_set_pragmas                   |
      | m20260411_000000_shop_settings                 |

  Scenario: No new migrations are introduced by Stage 3
    Given the post-Stage-3 MokumoApp graft
    When its migrations() are requested
    Then the returned list has exactly 8 entries

  # --- Invariant: runner never hardcodes a graft id ---

  Scenario: The runner accepts the graft id as a parameter
    Given two grafts "mokumo" and "garment" with disjoint migration sets
    When each graft runs its migrations independently
    Then the history table records each migration under its own graft id
    And neither graft's history interferes with the other

  Scenario: The runner source contains no literal "mokumo"
    Given the file crates/kikan/src/migrations/runner.rs
    Then it contains no string literal "mokumo"
    And the graft id reaches the runner only through function parameters
