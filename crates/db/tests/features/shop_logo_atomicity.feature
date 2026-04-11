Feature: Shop logo mutation and activity log atomicity

  Every logo mutation (upsert or delete) and its corresponding activity
  log entry are persisted together as a single atomic operation. Either
  both succeed or neither persists.

  # --- upsert_logo atomicity ---

  Scenario: Upserting a logo persists both the metadata and its activity entry
    Given an empty database
    When a PNG logo is upserted with epoch 1000000
    Then logo_extension should be "png"
    And logo_epoch should be 1000000
    And the activity log should contain a shop_settings "updated" entry
    And the activity payload should include action "shop_logo_uploaded"

  Scenario: Upserting twice overwrites extension and epoch
    Given an empty database
    When a PNG logo is upserted with epoch 1000000
    And a JPEG logo is upserted with epoch 2000000
    Then logo_extension should be "jpeg"
    And logo_epoch should be 2000000

  # --- delete_logo atomicity ---

  Scenario: Deleting a logo nulls the columns and logs the removal
    Given a PNG logo exists with epoch 1000000
    When the logo is deleted
    Then get_logo_info should return None
    And the activity log should contain a shop_settings "updated" entry
    And the activity payload should include action "shop_logo_deleted"

  # --- No-logo state ---

  Scenario: get_logo_info returns None when no logo has been set
    Given an empty database
    Then get_logo_info should return None
