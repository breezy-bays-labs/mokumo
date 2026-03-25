Feature: Every installed component has a story

  Scenario: Core UI components have stories
    Given Storybook is running
    Then each of the following components has at least one story:
      | component     |
      | Button        |
      | Card          |
      | Input         |
      | Label         |
      | Badge         |
      | Separator     |
      | Table         |
      | Tabs          |
      | Tooltip       |

  Scenario: App shell components have stories
    Given Storybook is running
    Then each of the following components has at least one story:
      | component     |
      | Sidebar       |
      | Sheet         |
      | Breadcrumb    |
      | AppSidebar    |
      | EmptyState    |

  Scenario: Form components have stories
    Given Storybook is running
    Then each of the following components has at least one story:
      | component     |
      | Checkbox      |
      | Radio Group   |
      | Select        |
      | Switch        |
      | Textarea      |

  Scenario: Overlay and feedback components have stories
    Given Storybook is running
    Then each of the following components has at least one story:
      | component      |
      | Dialog         |
      | Alert Dialog   |
      | Alert          |
      | Progress       |
      | Skeleton       |
      | Dropdown Menu  |
      | Toast          |
      | ConfirmDialog  |

  Scenario: Data display components have stories
    Given Storybook is running
    Then each of the following components has at least one story:
      | component     |
      | Avatar        |
      | Accordion     |
      | Collapsible   |
      | Scroll Area   |
      | Pagination    |

  Scenario: Accessibility panel shows warnings
    Given Storybook is showing a component story
    When I open the accessibility panel
    Then axe-core violations are displayed at warning level
