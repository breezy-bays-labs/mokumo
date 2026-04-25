Feature: Admin sign-in

  The admin sign-in screen is the entry point for an existing
  administrator. It accepts an email + password, surfaces the
  result of authentication clearly, and offers paths to first-time
  setup and password recovery when those are reachable.

  # --- Form structure ---

  @pr2a
  Scenario: Sign-in form shows the expected fields and actions
    Given I am on the admin sign-in screen
    Then I see an email field
    And I see a password field
    And I see a "Sign in" button
    And I see a "Forgot password?" link

  @pr2a
  Scenario: Branding tokens are applied to the sign-in chrome
    Given the platform reports a branding configuration
    When I open the admin sign-in screen
    Then the page shows the configured app name
    And the page shows the configured shop noun in body copy
    And the chrome surfaces use the branded color tokens

  # --- First-time setup affordance ---

  @pr2a @spans-layers
  Scenario: First-time setup link is shown when no admin exists
    Given the platform reports that no admin account exists
    When I open the admin sign-in screen
    Then I see a "First time setup?" link
    And the link points to the setup wizard

  @pr2a
  Scenario: First-time setup link is hidden once an admin exists
    Given the platform reports that an admin account exists
    When I open the admin sign-in screen
    Then I do not see a "First time setup?" link

  # --- Successful sign-in ---

  @needs-pr2b @spans-layers
  Scenario: Successful sign-in lands me on the overview
    Given an admin account exists
    When I sign in with the admin's correct email and password
    Then I am taken to the admin overview
    And the session cookie is scoped to the entire site

  @needs-pr2b
  Scenario: Sign-in redirects me back to the page I was trying to reach
    Given I tried to open a protected admin page while signed out
    And I was redirected to the sign-in screen
    When I sign in with correct credentials
    Then I am taken back to the page I originally tried to open

  # --- Failure paths ---

  @needs-pr2b
  Scenario: Wrong credentials show a generic failure message
    Given an admin account exists
    When I sign in with the admin's email but the wrong password
    Then I see a generic "Email or password is incorrect" message
    And the email and password fields keep their values
    And I am still on the sign-in screen

  @needs-pr2b
  Scenario: Repeated failures trigger a rate-limit message
    Given I have failed sign-in five times in a row
    When I submit the form again
    Then I see a "Too many attempts, try again shortly" message
    And the "Sign in" button is disabled until the cooldown ends

  @pr2a
  Scenario: Offline submission shows a self-healing reconnecting state
    Given I am on the admin sign-in screen
    And the platform is unreachable
    When I submit the form
    Then I see a self-healing banner that the connection is being retried
    And my form values are preserved

  # --- Permission landing ---

  @needs-pr2b @spans-layers
  Scenario: Signing in as a user without admin install role lands on the no-admin page
    Given my account exists but does not have admin install role
    When I sign in with correct credentials
    Then I am taken to the no-admin page
    And the page links me back to the shop

  # --- Session cookie landmine guard ---

  @needs-pr2b @spans-layers
  Scenario: Session cookie set during sign-in survives a navigation to the shop and back
    Given I have just signed in successfully
    When I navigate to a shop page outside the admin mount
    And I navigate back to an admin page
    Then I am still signed in
