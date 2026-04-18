@future
Feature: mokumo-server clap dispatch

  mokumo-server is both a long-running daemon (serve) and a one-shot
  CLI (diagnose, bootstrap, backup, --version). The dispatch keeps boot
  paths independent: one-shot subcommands must not open the UDS listener
  or take shared locks reserved for the daemon.

  # H1 decision in discover-decisions.md. Prevents regressions where
  # a trivial subcommand accidentally spawns the full daemon boot.

  # --- Version subcommand ---

  Scenario: --version prints the version and exits without opening UDS or DB
    Given a configured mokumo-server installation
    When mokumo-server --version runs
    Then the process exits with status 0
    And stdout matches the pattern "mokumo-server [0-9]+\\.[0-9]+\\.[0-9]+"
    And no UDS file was created
    And no meta.db connection was opened

  # --- Top-level help ---

  Scenario: Top-level --help advertises the four verbs without naming garage-shape
    When mokumo-server --help runs
    Then stdout lists the subcommands "serve", "diagnose", "bootstrap", "backup"
    And stdout does NOT mention "garage-shape" at the top level

  # --- Mode flag is a serve subflag, not a top-level verb ---

  Scenario: serve --help enumerates --mode values including garage-shape
    When mokumo-server serve --help runs
    Then stdout documents the --mode flag
    And stdout lists "garage-shape" as a valid value for --mode
    And stdout lists at least one other mode value

  Scenario: garage-shape is not reachable as a top-level subcommand
    When mokumo-server garage-shape runs
    Then the process exits with non-zero status
    And stderr indicates that "garage-shape" is not a recognized subcommand

  # --- One-shot subcommands do not linger ---

  Scenario Outline: One-shot subcommands exit promptly and do not open the UDS listener
    Given a configured mokumo-server installation
    When mokumo-server <subcommand> runs
    Then the process exits within 10 seconds
    And no UDS file was opened by the invocation

    Examples:
      | subcommand                                          |
      | --version                                           |
      | diagnose                                            |
      | diagnose --json                                     |
      | backup create /tmp/mokumo-backup.tar                |

  # --- Daemon mode opens UDS and blocks ---

  Scenario: serve opens the UDS listener and blocks until signaled
    Given a configured mokumo-server installation
    When mokumo-server serve is launched in the background
    Then within 2 seconds the UDS file exists at the configured path
    And the process continues running until signaled
