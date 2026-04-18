@future
Feature: mokumo-server bootstrap subcommand

  mokumo-server bootstrap provisions the first admin user on a fresh
  deployment without requiring the desktop first-launch wizard. It is
  the headless equivalent of the desktop bootstrap path and reaches the
  same kikan::control_plane::users::bootstrap_first_admin handler.

  # H5 decision in discover-decisions.md. Reachability is required for
  # garage-to-LAN deployments where no desktop is present.

  # --- Happy path ---

  Scenario: Bootstrap succeeds on a fresh meta database
    Given an empty meta.db and no admin user exists
    When mokumo-server bootstrap --email "founder@shop.example" --password-file pw.txt runs
    Then the process exits with status 0
    And stdout contains 10 recovery codes, one per line
    And an admin user "founder@shop.example" exists

  Scenario: Recovery codes are written to a file when --recovery-codes-file is supplied
    Given an empty meta.db and no admin user exists
    When mokumo-server bootstrap --email "founder@shop.example" --password-file pw.txt --recovery-codes-file codes.txt runs
    Then the process exits with status 0
    And stdout does not contain any recovery code
    And the file codes.txt contains 10 recovery codes, one per line

  # --- Password source ---

  Scenario: Password is read interactively from a TTY when --password-file is omitted
    Given an empty meta.db
    And a TTY is attached to stdin
    When mokumo-server bootstrap --email "founder@shop.example" runs interactively and receives a password via prompt
    Then the process exits with status 0
    And an admin user "founder@shop.example" exists

  Scenario: Bootstrap fails cleanly when stdin is not a TTY and --password-file is omitted
    Given an empty meta.db
    And stdin is not a TTY
    When mokumo-server bootstrap --email "founder@shop.example" runs
    Then the process exits with non-zero status
    And stderr contains the text "password required: supply --password-file or attach a TTY"

  # --- Idempotency guard ---

  Scenario: Bootstrap is rejected when any admin already exists
    Given an admin user "existing@shop.example" already exists
    When mokumo-server bootstrap --email "another@shop.example" --password-file pw.txt runs
    Then the process exits with non-zero status
    And stderr contains the text "ALREADY_BOOTSTRAPPED"
    And no user "another@shop.example" exists

  # --- Actor attribution ---

  Scenario: Bootstrap writes an activity log entry attributed to the reserved system actor
    Given an empty meta.db
    When mokumo-server bootstrap --email "founder@shop.example" --password-file pw.txt runs
    Then the activity log contains a "user.bootstrap" entry
    And that entry's actor_id equals the reserved ACTOR_SYSTEM_UID
