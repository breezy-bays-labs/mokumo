@wip
Feature: WebSocket disconnect banner

  When the server shuts down or the connection drops, employees see
  a clear banner with guidance instead of a broken page.

  Scenario: Banner appears when server sends shutdown message
    Given the WebSocket connection is established
    When the server sends a "server_shutting_down" message
    Then I see a banner "Server disconnected — reconnecting automatically. If this persists, check with your shop admin."

  Scenario: Reconnection indicator shows during backoff
    Given the WebSocket connection was lost
    When the client is attempting to reconnect
    Then I see a reconnection indicator

  Scenario: Banner clears when reconnected
    Given the WebSocket connection was lost
    And I see the disconnect banner
    When the client successfully reconnects
    Then the disconnect banner disappears
    And I see a "Reconnected" confirmation briefly

  Scenario: Banner appears on unexpected connection loss
    Given the WebSocket connection is established
    When the connection drops unexpectedly
    Then I see the disconnect banner
    And the client begins reconnecting automatically
