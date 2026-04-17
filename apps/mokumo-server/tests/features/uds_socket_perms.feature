@future
Feature: mokumo-server UDS socket permission enforcement

  mokumo-server serve exposes the admin control plane over a Unix domain
  socket. The socket file itself IS the capability — trust is gated by
  filesystem permissions. The binary refuses to start if the socket path
  exists with wrong mode or owner, and the correct-perm socket serves
  admin handlers. This is the only auth on the UDS transport: no cookies,
  no peer-cred.

  # Documented in adr-auth-security-under-cp-dp (UDS Auth subsection,
  # amended by PR #B). H2 decision in discover-decisions.md.

  # --- Positive path ---

  Scenario: mokumo-server serve binds the UDS at mode 0600
    Given a clean data directory
    When mokumo-server serve is launched
    Then the UDS file at the configured path exists
    And the UDS file mode is 0600
    And the UDS file is owned by the current user

  Scenario: A request over the UDS reaches a control plane handler
    Given mokumo-server serve is running with a 0600 socket
    When an HTTP GET "/admin/diagnostics" is sent over the UDS
    Then the response http status is 200
    And the response body contains a diagnostics report

  # --- Negative path (CQO requirement) ---

  Scenario: mokumo-server serve refuses to start if the socket path exists with wrong mode
    Given a pre-existing socket file at the configured path with mode 0644
    When mokumo-server serve is launched
    Then the process exits with non-zero status
    And stderr contains the text "socket permissions must be 0600"
    And no control plane handler is reachable over the UDS

  Scenario: mokumo-server serve refuses to start if the socket path exists with wrong owner
    Given a pre-existing socket file at the configured path owned by a different uid
    When mokumo-server serve is launched
    Then the process exits with non-zero status
    And stderr contains the text "socket owner mismatch"

  # --- Drift protection (OS-level expectation, documented) ---

  Scenario: A caller loses access when the socket's perms drift to 0644 at runtime
    Given mokumo-server serve is running with a 0600 socket
    When an external actor changes the socket mode to 0644
    And a non-owner attempts to open the socket
    Then the non-owner open fails with EACCES

  # --- Graceful shutdown ---

  Scenario: mokumo-server serve drains in-flight UDS requests within 5 seconds on SIGTERM
    Given mokumo-server serve is running with a 0600 socket
    And an in-flight request is being handled
    When SIGTERM is sent to the process
    Then the in-flight request completes
    And the process exits within 5 seconds
    And the UDS file is removed or unlinked
