Feature: Activity log visibility across Stage 3

  The activity log is platform-owned — the `activity_log` table and
  the read handler live in kikan — but the `action` column stores
  strings defined by the vertical. Existing rows written by the
  pre-Stage-3 binary must render identically after the Stage 3
  upgrade so that audit trails remain continuous for shop owners and
  for any external consumer reading the API response.

  Continuity anchors (byte-identical to pre-Stage-3):
  - `action` values: `"created"`, `"updated"`, `"soft_deleted"`,
    `"restored"`, `"login_success"`, `"login_failed"`,
    `"password_changed"`, `"setup_completed"`, `"password_reset"`,
    `"recovery_codes_regenerated"`.
  - wire field: `created_at` (RFC 3339 string).
  - wire field: `actor_id` (opaque string), `actor_type` (opaque string).

  Ordering hazard: the `activity_log.created_at` column is stored by
  the pre-Stage-3 migration with a DEFAULT of
  `strftime('%Y-%m-%dT%H:%M:%SZ', 'now')` — second precision, no
  fractional seconds (see
  `crates/db/src/migration/m20260324_000001_customers_and_activity.rs:60`).
  Any batch insert within the same second produces identical
  `created_at` values. The `id` column is `INTEGER PRIMARY KEY
  AUTOINCREMENT`, so id is strictly monotonic; `(created_at DESC,
  id DESC)` is the only stable ordering and must be preserved
  post-Stage-3.

  # --- Byte-for-byte continuity ---

  Scenario Outline: Historical action strings round-trip unchanged
    Given the activity_log contains a row with action "<action>"
    When the list_activity handler serializes that row
    Then the response entry's "action" field is exactly "<action>"

    Examples:
      | action                      |
      | created                     |
      | updated                     |
      | soft_deleted                |
      | restored                    |
      | login_success               |
      | password_changed            |
      | recovery_codes_regenerated  |

  Scenario: Historical payload JSON round-trips unchanged
    Given the activity_log contains a row whose payload is the JSON document
      """
      {"display_name":"Acme","email":"a@b.co"}
      """
    When the list_activity handler serializes that row
    Then the response payload deserializes to the same JSON document
    And the payload's keys appear in the same order as stored

  Scenario: Historical timestamps round-trip unchanged
    Given the activity_log contains a row with created_at "2025-11-02T14:30:00Z"
    When the list_activity handler serializes that row
    Then the response's "created_at" field is "2025-11-02T14:30:00Z"

  # --- Ordering and pagination ---

  Scenario: Activity entries are returned newest-first by created_at
    Given three activity_log rows with ids 101, 102, 103
    And their created_at values are "2025-11-02T14:30:00Z", "2025-11-02T14:30:01Z", "2025-11-02T14:30:02Z" respectively
    When the list_activity handler runs without a cursor
    Then the response lists row 103 first
    And row 101 last

  Scenario: Ties on created_at are broken by id descending
    Given two activity_log rows with created_at "2025-11-02T14:30:00Z"
    And the rows have ids 42 and 43 respectively
    When the list_activity handler runs without a cursor
    Then the response lists row 43 before row 42

  Scenario: Batch-inserted rows within the same second appear in reverse insertion order
    Given five activity_log rows inserted in a single transaction
    And all five rows share created_at "2025-11-02T14:30:00Z"
    And AUTOINCREMENT assigned ids 201 through 205 in insertion order
    When the list_activity handler runs without a cursor
    Then the response lists the rows in the order 205, 204, 203, 202, 201

  Scenario: Subsequent page continues the (created_at DESC, id DESC) ordering
    Given 11 activity_log rows with created_at "2025-11-02T14:30:00Z" and ids 301 through 311
    When the list_activity handler runs with page 2 and per_page 10
    Then the response contains exactly one row
    And that row has id 301

  Scenario Outline: Pagination honours the limit
    Given the activity_log contains 30 rows
    When the list_activity handler runs with limit <limit>
    Then the response contains <returned> entries
    And the total reports 30

    Examples:
      | limit | returned |
      | 10    | 10       |
      | 1     | 1        |
      | 30    | 30       |
      | 100   | 30       |

  # --- Platform owns the table; vertical owns the action vocabulary ---

  Scenario Outline: The handler does not interpret the action string
    Given the activity_log contains a row with action "<stored>"
    When the list_activity handler serializes that row
    Then the response succeeds
    And the response entry's "action" field is exactly "<stored>"

    Examples:
      | stored              |
      | garment_printed     |
      | legacy_action_name  |
      | created             |
