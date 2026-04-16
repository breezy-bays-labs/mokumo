@wip @desktop-only
Feature: Demo Reset Restart

  Resetting demo data shuts the server down, clears the database,
  and starts a fresh server on a new OS-assigned port. When the
  freshly assigned port differs from the previous one, the desktop
  shell navigates the webview to the new address so the app stays
  connected without user intervention.

  # Note: step definitions use a test harness that forces a specific
  # port on each restart call so port-change behaviour is deterministic.

  # --- Server is available again after reset ---

  Scenario: Server is running after demo data is reset
    Given the desktop app is running
    When demo data is reset
    Then the server is accepting requests on a loopback port

  # --- Webview follows the new port when it changes ---

  Scenario: Webview navigates to the new address when the port changes
    Given the desktop app is running on a known loopback port
    When demo data is reset and the restart binds a different port
    Then the webview is showing the app at the new port

  # --- No unnecessary reload when the port stays the same ---

  Scenario: Webview does not navigate when the restart port is unchanged
    Given the desktop app is running on a known loopback port
    When demo data is reset and the restart binds the same port
    Then the webview remains at the original address without reloading
