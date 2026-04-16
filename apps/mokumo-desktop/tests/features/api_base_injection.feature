@wip @desktop-only
Feature: API Base Injection

  The desktop shell communicates the server address to the web app
  before the interface loads. The OS assigns a fresh port each launch,
  so the app cannot know the address in advance — it must be delivered
  by the shell at startup.

  # --- Address injected before the app initialises ---

  Scenario: Server address is available before the interface initialises
    Given the desktop app has started on an OS-assigned loopback port
    When the webview begins loading the page
    Then the server address global is defined before the app interface mounts

  Scenario: Injected address reflects the OS-assigned port
    Given the desktop app has started on an OS-assigned loopback port
    When the webview loads the page
    Then the server address global is "http://127.0.0.1:{port}" where port matches the bound port
    And the port in the global is not a hardcoded value

  # --- Typed accessor ---

  Scenario: The accessor returns the injected address in desktop context
    Given the server address global is set to a loopback URL
    When the app calls the server address accessor
    Then the returned URL is the injected loopback address

  Scenario: The accessor returns the development default when the global is absent
    Given the server address global is not set
    When the app calls the server address accessor
    Then the returned URL is the default development server address

  # --- Webview can reach the server (V3 — ACL wildcard) ---

  Scenario: Webview request reaches the server at the assigned port
    Given the desktop app is running on an OS-assigned loopback port
    When the webview sends a request to the server health endpoint
    Then the request is permitted and the server responds
