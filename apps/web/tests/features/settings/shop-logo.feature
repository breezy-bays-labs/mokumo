@wip
Feature: Shop logo management in Settings

  Shop owners upload and remove a custom logo from the Settings page.
  The sidebar profile trigger reflects the current logo immediately.

  Background:
    Given I am logged in as an admin on the production profile

  @wip
  Scenario: Uploading a PNG logo updates the sidebar trigger
    Given I am on the shop settings page
    When I upload a valid PNG logo
    Then the sidebar profile trigger shows the custom logo

  @wip
  Scenario: Removing the logo restores the Store glyph
    Given a logo has been uploaded
    And I am on the shop settings page
    When I click the Remove logo button
    Then the sidebar profile trigger shows the Store glyph

  @wip
  Scenario: Uploading a GIF shows a validation error message
    Given I am on the shop settings page
    When I upload a GIF file as the logo
    Then I see the error "Only PNG, JPEG, or WebP files are accepted."

  @wip
  Scenario: Uploading an oversized file shows a validation error message
    Given I am on the shop settings page
    When I upload a file larger than 2 MB
    Then I see the error "File is too large. Max 2 MB."
