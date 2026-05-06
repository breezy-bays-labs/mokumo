Feature: Shop restore from backup

  A shop owner with an existing .db backup file can restore it on a fresh
  Mokumo install. The restore endpoint validates the file, copies it to the
  production slot, and triggers a server restart. This is a one-shot operation
  available only on first launch before any production database exists.

  # --- Guard: RestoreGuard ---

  Scenario: Restore is rejected when production database already exists
    Given a running server with an existing production database
    When a restore request is submitted with a valid Mokumo database
    Then the request is rejected with status 409
    And the error code is "production_db_exists"

  Scenario: Restore is rejected when not first launch
    Given a running server that has completed first-launch setup
    When a restore request is submitted with a valid Mokumo database
    Then the request is rejected with status 403

  # tracked: mokumo#814 — BDD harness gap (concurrency cannot be
  # reproduced from cucumber-rs without a shared AppState handle);
  # backfill needed at unit-test layer
  @allow.skipped
  Scenario: Concurrent restore attempts are rejected
    Given a running server on first launch with no production database
    And a restore request is already in progress
    When a second restore request arrives simultaneously
    Then the second request is rejected with status 409
    And the error code is "restore_in_progress"

  # --- Validation: check_application_id ---

  Scenario: Non-SQLite file is rejected
    Given a running server on first launch with no production database
    When a restore request is submitted with a plain text file
    Then the request is rejected with status 422
    And the error code is "not_mokumo_database"

  # 0x12345678 fits in signed i32 and is neither 0 (unstamped) nor
  # KIKAN_APPLICATION_ID (= 'MKMO' = 0x4D4B4D4F). Earlier revisions used
  # 0xDEADBEEF, but SQLite silently truncates `PRAGMA application_id`
  # values that overflow signed i32, so the file ended up stamped 0 and
  # was accepted as "not yet stamped" — masking the rejection path.
  Scenario: SQLite file with wrong application_id is rejected
    Given a running server on first launch with no production database
    When a restore request is submitted with a SQLite file whose application_id is 0x12345678
    Then the request is rejected with status 422
    And the error code is "not_mokumo_database"

  Scenario: SQLite file with application_id 0 is accepted
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database with application_id 0
    Then the request succeeds with status 200

  Scenario: SQLite file with application_id 0x4D4B4D4F is accepted
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database with application_id 0x4D4B4D4F
    Then the request succeeds with status 200

  # --- Validation: integrity_check ---

  # A truncated file fails SQLite's open() before any pragma queries run,
  # so the identity-check (step 1) rejects it as not_mokumo_database
  # rather than reaching integrity_check (step 2). This scenario pins
  # that contract so the two paths stay distinguishable.
  Scenario: Truncated SQLite file is rejected by identity check
    Given a running server on first launch with no production database
    When a restore request is submitted with a truncated SQLite file
    Then the request is rejected with status 422
    And the error code is "not_mokumo_database"

  # Exercises the integrity_check path: real Mokumo DB whose middle
  # bytes are scrambled — opens cleanly, application_id matches, but
  # `PRAGMA integrity_check` returns non-"ok".
  Scenario: Corrupt database file is rejected by integrity check
    Given a running server on first launch with no production database
    When a restore request is submitted with a corrupted Mokumo database
    Then the request is rejected with status 422
    And the error code is "database_corrupt"

  # --- Validation: schema_compatibility ---

  Scenario: Database from newer Mokumo version is rejected
    Given a running server on first launch with no production database
    When a restore request is submitted with a database containing unknown migration versions
    Then the request is rejected with status 422
    And the error code is "schema_incompatible"

  Scenario: Database from older Mokumo version is accepted
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database from an older version
    Then the request succeeds with status 200

  # --- Copy mechanism ---

  Scenario: Successful restore copies database to production slot
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database
    Then the database is copied to the production slot
    And the production database matches the source file

  Scenario: Restore writes active_profile to disk
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database
    Then the active_profile file contains "production"

  # --- Restart ---

  Scenario: Successful restore writes restart sentinel
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database
    Then a .restart sentinel file exists in the data directory

  Scenario: Successful restore triggers graceful shutdown
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database
    Then the server initiates a graceful shutdown

  Scenario: Restore responds before shutdown completes
    Given a running server on first launch with no production database
    When a restore request is submitted with a valid Mokumo database
    Then the response is received with status 200
    And the response body indicates success

  # --- Validate endpoint ---

  Scenario: Validate endpoint returns candidate info for valid file
    Given a running server on first launch with no production database
    When a validate request is submitted with a valid Mokumo database
    Then the response is received with status 200
    And the response contains the file name and size
    And the response contains the schema version
    And the response indicates the file is valid

  Scenario: Validate endpoint rejects invalid file without copying
    Given a running server on first launch with no production database
    When a validate request is submitted with a non-Mokumo SQLite file
    Then the request is rejected with status 422
    And the error code is "not_mokumo_database"
    And no file is copied to the production slot

  Scenario: Validate endpoint shares guards with restore
    Given a running server with an existing production database
    When a validate request is submitted with a valid Mokumo database
    Then the request is rejected with status 409
    And the error code is "production_db_exists"

  # --- Rate limiting ---

  Scenario: Rate limit is enforced across both endpoints
    Given a running server on first launch with no production database
    When 5 restore or validate requests are submitted within one hour
    Then the 6th request is rejected with status 429
    And the error code is "rate_limited"

  # --- Body parse errors (wire-code contract — see mokumo#701) ---
  #
  # The Hurl smoke suite cannot reach these paths under the demo harness
  # because RestoreGuard fires before body parsing. These BDD scenarios
  # boot a first-launch server (no production DB), so the guard passes and
  # extract_candidate executes — letting us pin the parse_error wire shape.

  Scenario: Restore endpoint rejects unparseable JSON body with parse_error
    Given a running server on first launch with no production database
    When a restore request is submitted with an unparseable JSON body
    Then the request is rejected with status 400
    And the error code is "parse_error"

  Scenario: Restore endpoint rejects unsupported Content-Type with parse_error
    Given a running server on first launch with no production database
    When a restore request is submitted with Content-Type "text/plain"
    Then the request is rejected with status 400
    And the error code is "parse_error"

  Scenario: Restore endpoint rejects multipart with no file field
    Given a running server on first launch with no production database
    When a restore request is submitted with a multipart body that has no file field
    Then the request is rejected with status 400
    And the error code is "parse_error"

  Scenario: Validate endpoint rejects unparseable JSON body with parse_error
    Given a running server on first launch with no production database
    When a validate request is submitted with an unparseable JSON body
    Then the request is rejected with status 400
    And the error code is "parse_error"

  Scenario: Validate endpoint rejects unsupported Content-Type with parse_error
    Given a running server on first launch with no production database
    When a validate request is submitted with Content-Type "text/plain"
    Then the request is rejected with status 400
    And the error code is "parse_error"

  # --- Rollback on partial failure ---

  # tracked: mokumo#814 — BDD harness gap (chmod 0o444 on the file does
  # not stop atomic write-via-rename when the parent dir is writable);
  # backfill needs a real read-only mount or an injectable fs-failure seam
  @allow.skipped
  Scenario: Failed profile write rolls back copied database
    Given a running server on first launch with no production database
    And the active_profile file location is read-only
    When a restore request is submitted with a valid Mokumo database
    Then the request fails with status 500
    And no production database file exists
