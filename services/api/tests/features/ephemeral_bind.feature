Feature: Ephemeral Loopback Bind

  The desktop server binds to the local loopback address on an
  OS-assigned port. Only software running on the same machine
  can reach it, and the port is chosen fresh at each launch.

  # --- Bind address ---

  Scenario: Desktop server binds to loopback only
    When the desktop server requests an ephemeral loopback port
    Then the bound address is on 127.0.0.1
    And the server is not reachable from other network interfaces

  Scenario: Desktop server receives an OS-assigned port
    When the desktop server requests an ephemeral loopback port
    Then the assigned port is greater than zero
    And no fixed or preferred port was requested

  # --- Readable address ---

  Scenario: Bound address is readable immediately after bind
    When the desktop server requests an ephemeral loopback port
    Then the full socket address (host and port) can be read from the listener
    And the address host is 127.0.0.1
    And the address port is the same as the OS-assigned port

  # --- Independence from the fixed-port fallback ---

  Scenario: Ephemeral bind succeeds even when the default ports are occupied
    Given ports 6565 through 6575 are already in use
    When the desktop server requests an ephemeral loopback port
    Then the bind succeeds
    And the assigned port is outside the 6565-6575 range
