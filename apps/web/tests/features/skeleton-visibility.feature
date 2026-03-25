Feature: Skeleton visibility in Storybook

  Skeleton elements use bg-muted which can be invisible against a white canvas.
  Stories must provide contrasting backgrounds so skeletons are visible.

  Scenario: Skeleton is visible against the story background in light mode
    Given Storybook is showing the Skeleton Default story in light mode
    When the story renders
    Then the skeleton element is visually distinguishable from the background

  Scenario: Skeleton is visible against the story background in dark mode
    Given Storybook is showing the Skeleton Default story in dark mode
    When the story renders
    Then the skeleton element is visually distinguishable from the background
