Feature: Toast notifications

  Toast notifications provide feedback after user actions.
  Four variants (success, error, warning, info) styled with OKLCH design tokens.

  Scenario: Success toast appears with correct styling
    Given Storybook is showing the Toast Success story
    When a success toast is triggered
    Then a toast notification is visible
    And the toast has success variant styling

  Scenario: Error toast appears with correct styling
    Given Storybook is showing the Toast Error story
    When an error toast is triggered
    Then a toast notification is visible
    And the toast has error variant styling

  Scenario: Warning toast appears with correct styling
    Given Storybook is showing the Toast Warning story
    When a warning toast is triggered
    Then a toast notification is visible
    And the toast has warning variant styling

  Scenario: Info toast appears with correct styling
    Given Storybook is showing the Toast Info story
    When an info toast is triggered
    Then a toast notification is visible
    And the toast has info variant styling

  Scenario: Toast can be dismissed manually
    Given Storybook is showing the Toast Success story
    When a success toast is triggered
    And I click the close button on the toast
    Then the toast notification is no longer visible

  Scenario: Multiple toasts stack
    Given Storybook is showing the Toast Stacked story
    When multiple toasts are triggered
    Then more than one toast notification is visible
