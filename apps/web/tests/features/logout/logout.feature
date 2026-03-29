Feature: Logout session destroy

  Clicking "Log out" should destroy the server-side session before redirecting
  to the login page.

  Scenario: Logout calls the backend endpoint and redirects to login
    Given the app shell is loaded
    When the user opens the avatar popover
    And the user clicks "Log out"
    Then a POST request was sent to "/api/auth/logout"
    And the page navigates to "/login"

  Scenario: Logout shows error toast when the API call fails
    Given the app shell is loaded
    And the logout endpoint will return a server error
    When the user opens the avatar popover
    And the user clicks "Log out"
    Then an error toast is shown with text "Logout failed"
    And the page does not navigate to "/login"
