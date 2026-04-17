@future
Feature: Garment-vertical graft boundary

  The mokumo-garment crate is the first consumer of the kikan
  platform via the `Graft` trait. This feature specifies the
  contract at the boundary: mokumo-garment implements every Graft
  surface, owns its domain language entirely, and — crucially —
  the kikan crate never mentions the garment vertical by name.

  The I1 CI script (`scripts/check-i1-domain-purity.sh`) and the
  I4 workspace dependency check enforce the kikan-side half of
  this invariant mechanically; these scenarios capture the
  *intent* and the vertical-side obligations that no script
  checks. Migration-name continuity is covered in
  `kikan/tests/features/migration_replay_safety.feature`; this
  feature asserts only the *count* and the Graft surface.

  # --- Graft trait implementation ---

  Scenario: MokumoApp implements Graft
    Given the mokumo-garment crate compiles
    Then the trait kikan::Graft is implemented for MokumoApp
    And the associated type AppState resolves to MokumoAppState

  Scenario: Graft id is stable across Stage 3
    Given MokumoApp implements Graft
    When its id is requested
    Then the returned GraftId equals kikan::GraftId::new("mokumo")

  Scenario: build_state constructs MokumoAppState from an EngineContext
    Given MokumoApp implements Graft
    When build_state is called with a valid EngineContext
    Then the result is a ready-to-serve MokumoAppState
    And the state embeds the supplied EngineContext

  Scenario: Migrations list has the same length as the pre-Stage-3 history
    Given MokumoApp implements Graft
    When migrations() is requested
    Then the returned list has exactly 8 entries
    # Name-by-name continuity: see migration_replay_safety.feature

  # --- Data-plane routes ---

  Scenario: data_plane_routes returns a Router parameterised over MokumoAppState
    Given MokumoApp implements Graft
    When data_plane_routes is requested
    Then the returned Router is typed Router<MokumoAppState>
    And the router does not attach platform layers itself

  Scenario Outline: Router exposes the pre-Stage-3 customer route
    Given MokumoApp implements Graft
    When data_plane_routes is requested
    Then the returned Router has a "<method>" route at "<path>"

    Examples:
      | method | path                      |
      | GET    | /api/customers            |
      | POST   | /api/customers            |
      | GET    | /api/customers/{id}       |
      | PUT    | /api/customers/{id}       |
      | DELETE | /api/customers/{id}       |
      | PATCH  | /api/customers/{id}/restore |

  # --- Kikan purity (I1 + I4 restated as spec anchors) ---

  Scenario Outline: The I1 CI gate rejects garment identifiers in kikan
    Given the script scripts/check-i1-domain-purity.sh is run
    When the kikan crate source contains the identifier "<forbidden>"
    Then the script exits non-zero

    Examples:
      | forbidden   |
      | customer    |
      | garment     |
      | quote       |
      | invoice     |
      | print_job   |

  Scenario: The I4 workspace dependency check runs on every PR
    Given the workspace manifest
    Then mokumo-garment lists kikan in its dependencies
    And kikan does not list mokumo-garment in its dependencies
    And CI fails any PR that adds mokumo-garment as a kikan dependency
