@wip
Feature: mDNS retry with backoff

  When mDNS registration fails (common on ~20-30% of small business
  networks), Mokumo retries automatically with increasing intervals
  so LAN discovery recovers without manual intervention.

  Scenario: mDNS retries after initial failure
    Given the server is started with "--host 0.0.0.0"
    And mDNS registration fails
    When 60 seconds elapse
    Then mDNS registration is retried

  Scenario Outline: Retry interval increases with backoff
    Given mDNS registration has failed
    When retry attempt <attempt> fails
    Then the next retry occurs after <delay> seconds

    Examples:
      | attempt | delay |
      | 1       | 120   |
      | 2       | 300   |
      | 3       | 300   |

  Scenario: Retry interval caps at 5 minutes
    Given mDNS registration has failed multiple times
    When the backoff reaches 300 seconds
    Then subsequent retries remain at 300 second intervals

  Scenario: Successful retry stops the retry loop
    Given mDNS registration has failed and retries are active
    When a retry succeeds
    Then the retry loop is cancelled
    And the server status changes to mDNS active

  Scenario: mDNS retry is cancelled on server shutdown
    Given mDNS registration has failed and retries are active
    When the server begins shutting down
    Then the retry task is cancelled
    And no further retries are attempted

  Scenario: Server info reflects mDNS recovery
    Given mDNS registration failed at startup
    And retries are active
    When a retry succeeds
    Then the server info endpoint shows mDNS is active
    And the LAN URL is now available

  Scenario: Shutdown during an active retry attempt completes without hanging
    Given mDNS registration has failed and a retry is in-flight
    When the server begins shutting down
    Then the in-flight retry is cancelled
    And the server shuts down within the drain timeout
