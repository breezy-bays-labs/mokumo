Feature: First Run Detection

  Mokumo detects whether initial setup has been completed so it
  can guide new shop owners through the setup wizard.

  @allow.skipped
  Scenario: Fresh database reports setup incomplete
    Given a freshly initialized database
    When setup status is checked
    Then setup is reported as incomplete

  @allow.skipped
  Scenario: Completed setup is remembered
    Given the setup wizard has been completed
    When setup status is checked
    Then setup is reported as complete
