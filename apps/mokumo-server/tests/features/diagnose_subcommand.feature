@future
Feature: mokumo-server diagnose subcommand

  mokumo-server diagnose is a one-shot health check that reports on the
  operator-visible state needed to decide whether the binary is functioning
  correctly. It is binding for M0 per Commitment 3 of
  adr-tauri-http-not-ipc.

  # H8 decision in discover-decisions.md. The report mirrors
  # kikan::control_plane::diagnostics::report() called in-process.

  # --- Human-readable default ---

  Scenario: Diagnose produces a human-readable report by default
    Given a working mokumo-server deployment
    When mokumo-server diagnose runs
    Then the process exits with status 0
    And stdout contains sections for "Socket", "Meta database", "Profiles", "Backups", "Disk"
    And stdout contains the UDS path and mode
    And stdout contains the meta.db path and size

  # --- JSON output for scripting ---

  Scenario: Diagnose emits structured JSON when --json is supplied
    Given a working mokumo-server deployment
    When mokumo-server diagnose --json runs
    Then the process exits with status 0
    And stdout is a single JSON object
    And the object has key "uds_path"
    And the object has key "uds_mode"
    And the object has key "meta_db_ok"
    And the object has key "profiles"
    And the object has key "disk_free_bytes"
    And the object has key "last_backup_at"
    And the object has key "migration_status"
    And each entry in "profiles" has keys "name", "path", "size_bytes", "wal_state"

  # --- Exit codes signal severity ---

  Scenario: Diagnose exits 1 when the deployment is degraded but not broken
    Given a working mokumo-server deployment
    And the last backup is older than 7 days
    When mokumo-server diagnose runs
    Then the process exits with status 1
    And stdout contains a "degraded" section describing the stale backup

  Scenario: Diagnose exits 2 when the deployment cannot serve requests
    Given a mokumo-server deployment
    And the meta.db file is missing
    When mokumo-server diagnose runs
    Then the process exits with status 2
    And stdout contains a "unhealthy" section describing the missing meta.db

  # --- Does not require a running daemon ---

  Scenario: Diagnose runs without opening the UDS listener
    Given mokumo-server serve is NOT running
    When mokumo-server diagnose runs
    Then the process exits with status 0
    And no UDS file was created by the diagnose invocation
