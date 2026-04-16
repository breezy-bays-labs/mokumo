@future
Feature: Opaque application state for the garment vertical

  `MokumoAppState` is the vertical's `Graft::AppState` associated
  type. To preserve the platform boundary (golden rule #1), the
  struct composes exactly one platform handle — `kikan::EngineContext`
  — plus vertical-owned repositories. No database pool, no tenancy
  handle, no session store, and no activity writer may appear as a
  direct field. Handlers reach platform services by borrowing them
  from the embedded `EngineContext` via `FromRef`.

  This feature specifies the *shape* of `MokumoAppState` rather than
  its behavior. Its scenarios are automatable with a `syn`-based
  structural test that parses the struct definition and walks the
  fields; see Automation Notes for Kit.

  # --- Permitted composition ---

  Scenario: MokumoAppState embeds EngineContext
    Given the definition of MokumoAppState
    Then the struct contains exactly one field of type "kikan::EngineContext"

  Scenario: MokumoAppState derives FromRef for axum extractors
    Given the definition of MokumoAppState
    Then it derives FromRef
    And an axum handler taking State<kikan::EngineContext> compiles
        against a Router<MokumoAppState>

  Scenario: EngineContext is cheaply cloneable
    Given the definition of kikan::EngineContext
    Then it derives Clone
    And every field of EngineContext is either Arc<T>, a native-Copy type,
        or a wrapper whose Clone is O(1)
    # FromRef requires Clone on the extracted type; an expensive Clone
    # would fire on every request and silently regress latency.

  Scenario: Remaining fields are vertical repository handles
    Given the definition of MokumoAppState
    Then every non-EngineContext field has a type path that begins
        with "mokumo_garment::"

  # --- Forbidden composition ---

  Scenario Outline: MokumoAppState exposes no platform primitive as a direct field
    Given the definition of MokumoAppState
    Then no field has type "<forbidden_type>"

    Examples:
      | forbidden_type                              |
      | sea_orm::DatabaseConnection                 |
      | std::sync::Arc<sea_orm::DatabaseConnection> |
      | sqlx::SqlitePool                            |
      | kikan::Tenancy                              |
      | std::sync::Arc<kikan::Tenancy>              |
      | kikan::Sessions                             |
      | std::sync::Arc<kikan::Sessions>             |
      | std::sync::Arc<dyn kikan::ActivityWriter>   |
