Feature: WebSocket client connection

  Scenario: Client connects to the server
    Given the server is available
    When the client opens a WebSocket connection
    Then the connection is established

  Scenario: Client receives a broadcast event
    Given the client is connected
    When the server broadcasts a "customer.created" event
    Then the client dispatches the event to the application
    And the event has type "customer.created"

  Scenario Outline: Client reconnect backoff increases with each attempt
    Given the client is connected
    When the connection is lost
    Then after attempt <attempt> the base wait time is <delay> seconds

    Examples:
      | attempt | delay |
      | 1       | 1     |
      | 2       | 2     |
      | 3       | 4     |
      | 6       | 30    |
      | 7       | 30    |

  Scenario: Client re-fetches data after reconnecting
    Given the client was disconnected
    When the client successfully reconnects
    Then the client notifies the application to refresh its data
    And subsequent reconnections start with the shortest wait time

  Scenario: Client receives a server-initiated close
    Given the client is connected
    When the server sends a close frame
    Then the client begins reconnecting
