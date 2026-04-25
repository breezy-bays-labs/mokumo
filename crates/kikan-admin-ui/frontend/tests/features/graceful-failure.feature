Feature: Graceful failure across the admin chrome

  Every authenticated screen reuses a small set of shared
  components for loading, error, empty, self-healing, and
  destructive-confirm states. This feature pins the contract those
  components must honor everywhere they appear, so individual
  screens can rely on consistent behavior instead of inventing
  their own.

  # --- LoadingState ---

  @pr2a
  Scenario: Loading state shows a skeleton matching the final layout
    Given a screen is fetching its initial data
    Then the loading state shows a skeleton
    And the skeleton's regions match the regions of the final layout
    And the skeleton replaces the content without shifting it on resolve

  @pr2a
  Scenario: Loading state announces itself to assistive technology
    Given a screen is fetching its initial data
    Then the loading state is announced to assistive technology
    And no spinner-only message is used

  # --- ErrorState ---

  @pr2a
  Scenario: 5xx response shows a retry-able error state
    Given a screen request returns a 5xx response
    Then I see an error state explaining the request failed
    And I see a "Try again" button
    And clicking "Try again" re-issues the request

  @needs-pr2b @spans-layers
  Scenario: 401 response signs me out and redirects to sign-in with a return path
    Given I am on a protected screen
    When a screen request returns a 401 response
    Then I am redirected to the admin sign-in screen
    And the original screen path is preserved as a return target

  @needs-pr2b
  Scenario: 403 response shows a "You don't have access" error state
    Given a screen request returns a 403 response
    Then I see a "You don't have access" error state
    And I do not see a "Try again" button

  @needs-pr2b
  Scenario: 429 response shows a countdown until the request can be retried
    Given a screen request returns a 429 response with a Retry-After hint
    Then I see an error state explaining the request was rate-limited
    And I see a countdown until I can retry
    And the "Try again" button is disabled until the countdown ends

  # --- EmptyState ---

  @pr2a
  Scenario: Empty state teaches what the screen will show once it has data
    Given a list screen has no items yet
    Then I see an empty state
    And the empty state explains what items will appear here
    And the empty state offers the primary action to create or import an item

  @pr2a
  Scenario: Empty state never shows a generic "no data" message
    Given a list screen has no items yet
    Then the empty state copy is specific to that screen
    And the copy is not the literal string "No data"

  # --- SelfHealingBanner ---

  @pr2a
  Scenario: Going offline shows a self-healing reconnecting banner
    Given I am on a screen
    When the platform becomes unreachable
    Then I see a self-healing banner that the connection is being retried
    And the banner does not require me to dismiss it manually

  @pr2a
  Scenario: Coming back online dismisses the banner without a page reload
    Given the self-healing banner is showing
    When the platform becomes reachable again
    Then the banner is dismissed automatically
    And the page is not reloaded

  @pr2a
  Scenario: Self-healing banner shows the next retry attempt
    Given the self-healing banner is showing
    Then the banner shows when the next retry will happen
    And the time-until-retry updates as it counts down

  # --- DestructiveConfirmModal: T1 plain confirm ---

  @pr2a
  Scenario: T1 plain confirm asks for an explicit confirmation
    Given a screen offers a destructive action that uses the T1 confirmation
    When I trigger the destructive action
    Then I see a confirmation modal
    And I see a clear description of what will happen
    And I see "Confirm" and "Cancel" buttons
    And the confirm button is enabled

  @pr2a
  Scenario: T1 plain confirm cancels cleanly
    Given the T1 confirmation modal is open
    When I click "Cancel"
    Then the modal closes
    And no destructive action is performed

  # --- DestructiveConfirmModal: T2 typed-name confirm ---

  @pr2a
  Scenario: T2 typed confirm requires the operator to type the target's name
    Given a screen offers a destructive action that uses the T2 confirmation
    When I trigger the destructive action
    Then I see a confirmation modal that names the target
    And the confirm button is disabled
    And there is a field asking me to type the target's name to confirm

  @pr2a
  Scenario: T2 typed confirm enables the confirm button only on an exact match
    Given the T2 confirmation modal is open for a target named "kiln-room"
    When I type "kiln-roo" into the confirmation field
    Then the confirm button is still disabled
    When I type the trailing "m"
    Then the confirm button becomes enabled

  @pr2a
  Scenario: T2 typed confirm rejects case mismatches
    Given the T2 confirmation modal is open for a target named "kiln-room"
    When I type "Kiln-Room" into the confirmation field
    Then the confirm button is still disabled

  # --- Boundary reload guard ---

  @needs-pr2b @spans-layers
  Scenario: Crossing the admin boundary and back does not lose the session
    Given I am signed in to the admin
    When I navigate to a path outside the admin mount
    And I navigate back to an admin path
    Then I am still signed in
    And no error state is shown
