Feature: Engine/UI version mismatch banner

  The admin UI bakes the `api_version` it was built against at compile
  time. On boot it fetches `GET /api/kikan-version` and compares. On
  mismatch it renders a non-blocking banner; on match no banner renders.
  This protects operators running `mokumo-server --spa-dir <path>` or
  future `kikan`-as-crates.io consumers against silent engine/UI drift
  (issue #502).

  Scenario: Banner renders when server api_version diverges from the UI build
    Given the server reports kikan api_version "99.0.0"
    When I open the admin UI
    Then a version-mismatch banner is visible
    And the banner names both the UI version and the server version

  Scenario: Banner stays hidden when server api_version matches the UI build
    Given the server reports the same kikan api_version the UI was built for
    When I open the admin UI
    Then no version-mismatch banner is visible
