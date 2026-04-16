@integration
Feature: ApalisScheduler persists jobs and uses SQLite storage
  As a vertical scheduling automatic backups
  I want jobs persisted to SQLite for reliability
  So that scheduled work survives process restarts

  Background:
    Given a tempfile SQLite database
    And an ApalisScheduler backed by the database

  Scenario: ApalisScheduler health check succeeds
    When check is called
    Then check returns Ok

  Scenario: A job can be scheduled with a delay
    When schedule_after is called with payload name "backup" delay 3600s
    Then a JobId is returned

  Scenario: ApalisScheduler storage survives reconnection
    When schedule_after is called with payload name "backup" delay 3600s
    And a new ApalisScheduler is constructed with the same database
    Then check returns Ok on the new scheduler
