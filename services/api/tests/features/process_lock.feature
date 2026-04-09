@wip
Feature: Single-instance process lock

  Mokumo uses a file lock to prevent two server instances from running
  simultaneously, which would corrupt the SQLite database. The lock file
  stores server info so conflict messages are actionable.

  # Lock acquisition and info storage

  Scenario: Server writes port info to lock file after acquiring lock
    Given no other Mokumo server is running
    When the server starts on port 6565
    Then the lock file contains "port=6565"

  Scenario: Server writes fallback port to lock file
    Given no other Mokumo server is running
    And port 6565 is already in use
    When the server starts
    Then the lock file contains the actual bound port

  # Server launch conflict

  Scenario: Second server instance is blocked with actionable message
    Given a Mokumo server is running on port 6565
    When a second server instance attempts to start
    Then it exits with error containing "already running on port 6565"
    And the error message suggests checking the system tray
    And the error message includes the URL "http://localhost:6565"

  Scenario: Conflict message includes fallback port when used
    Given a Mokumo server is running on port 6567
    When a second server instance attempts to start
    Then it exits with error containing "already running on port 6567"

  # Destructive command gating

  Scenario: reset-db is blocked while server is running
    Given a Mokumo server is running on port 6565
    When the "reset-db" command is executed
    Then it exits with error containing "Cannot reset database while the server is running"
    And the error message includes "port 6565"
    And the error message suggests stopping the server first

  # Non-destructive commands bypass lock

  Scenario: reset-password works while server is running
    Given a Mokumo server is running on port 6565
    When the "reset-password" command is executed
    Then it does not check the process lock
    And the command proceeds normally

  # Lock release

  Scenario: Lock is released when server shuts down cleanly
    Given a Mokumo server is running
    When the server shuts down
    Then the lock file is no longer locked
    And a new server instance can start

  Scenario: Server starts when lock file exists but lock is not held
    Given a previous server crashed leaving a lock file on disk
    And the file lock is not held (kernel released it)
    When a new server instance starts
    Then it acquires the lock successfully
    And the server starts normally
