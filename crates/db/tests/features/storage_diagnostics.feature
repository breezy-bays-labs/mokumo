Feature: Database storage diagnostics

  The database diagnostic function reports fragmentation ratio and WAL
  file size so that health checks and the doctor CLI can detect when a
  VACUUM is advisable and how much write-ahead log has accumulated.

  Scenario: Non-fragmented database is not flagged for vacuum
    Given a database with no deleted rows
    When storage diagnostics are collected
    Then vacuum_needed is false

  Scenario: Heavily fragmented database is flagged for vacuum
    Given a database where more than 20 percent of pages are free
    When storage diagnostics are collected
    Then vacuum_needed is true

  Scenario: Database at exactly 20 percent free pages is not flagged
    Given a database where exactly 20 percent of pages are free
    When storage diagnostics are collected
    Then vacuum_needed is false

  Scenario: WAL size is zero when no WAL file exists
    Given a database with no WAL file present
    When storage diagnostics are collected
    Then wal_size_bytes is 0

  Scenario: WAL size reflects the WAL file on disk
    Given a database with an active WAL file of known size
    When storage diagnostics are collected
    Then wal_size_bytes matches the size of the WAL file

  Scenario: Empty database does not flag vacuum needed
    Given an empty newly created database
    When storage diagnostics are collected
    Then vacuum_needed is false
