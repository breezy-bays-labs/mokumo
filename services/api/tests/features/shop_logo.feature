Feature: Shop logo upload, retrieval, and removal

  Shop owners can upload a custom logo for their shop. The logo is
  displayed in the sidebar profile switcher and falls back to a Store
  glyph when none is set. Only the production profile supports logo
  management; demo mode is read-only.

  Background:
    Given the API server is running

  # --- Upload ---

  Scenario: Uploading a valid PNG logo succeeds
    When I upload a valid PNG logo
    Then the response status should be 204

  Scenario: Setup status includes a logo_url after upload
    When I upload a valid PNG logo
    And I request GET "/api/setup-status"
    Then the response status should be 200
    And the logo_url should contain "/api/shop/logo"
    And the logo_url should contain "v="

  Scenario: Uploading a GIF is rejected with logo_format_unsupported
    When I upload a GIF file as the logo
    Then the response status should be 422
    And the error code should be "logo_format_unsupported"

  Scenario: Uploading a file that is too large is rejected with logo_too_large
    When I upload an oversized logo file
    Then the response status should be 422
    And the error code should be "logo_too_large"

  Scenario: Posting multipart with no logo field is rejected with missing_field
    When I post multipart with no logo field
    Then the response status should be 400
    And the error code should be "missing_field"

  # --- Retrieval ---

  Scenario: GET logo returns 200 with correct Content-Type after upload
    Given a logo has been uploaded
    When I request GET "/api/shop/logo"
    Then the response status should be 200
    And the response Content-Type should contain "image/png"

  Scenario: GET logo returns 404 when no logo is set
    When I request GET "/api/shop/logo"
    Then the response status should be 404
    And the error code should be "shop_logo_not_found"

  # --- Delete ---

  Scenario: Deleting the logo succeeds and subsequent GET returns 404
    Given a logo has been uploaded
    When I delete the logo
    Then the response status should be 204
    When I request GET "/api/shop/logo"
    Then the response status should be 404

  Scenario: Deleting when no logo is set returns 404
    When I delete the logo
    Then the response status should be 404
    And the error code should be "shop_logo_not_found"
