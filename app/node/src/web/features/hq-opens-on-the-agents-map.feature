Feature: HQ opens on the agents map
  The owner decided (M6, 7 Nov 2026) that the agents map is HQ's primary view: who is working on
  what, what is blocked, and what waits on the owner. Every way into HQ lands there: a fresh
  visit, a reload, the old agents screen, links and choices kept from the old tabs. The owner's
  decisions stand beside the map, and the studio is a secondary place of its own.

  Background:
    Given HQ has John Doe working on "example task" in "example project"
    And the desk has a question "Should the example task go on?"

  Scenario: A fresh visit opens the map with the decisions beside it
    When the owner opens the console
    Then HQ shows the agents map
    And the owner's decisions are beside the map
    And the studio is not drawn on the map

  Scenario: A browser that remembered an old tab still opens the map
    Given this browser remembers the old "manage" tab
    When the owner opens the console
    Then HQ shows the agents map
    And this browser no longer remembers an old tab

  Scenario: The old agents screen opens HQ's map
    Given the owner is on "Chess league"
    When the owner follows the old agents map link
    Then HQ shows the agents map

  Scenario: Links from the old tabs land where their things now live
    When the owner follows an old link to the "work" tab
    Then HQ shows the agents map
    When the owner follows an old link to the "team" tab
    Then HQ shows "Projects & team" with a way back to the map
    When the owner follows an old link to the "office" tab
    Then HQ shows "Office" with a way back to the map

  Scenario: A secondary place is a page with a way back
    When the owner opens "Health"
    Then HQ shows "Health" with a way back to the map
    When the owner presses Escape
    Then HQ shows the agents map

  Scenario: The map is never blank when HQ's map does not answer
    Given HQ's map does not answer
    When the owner opens the console
    Then the map says HQ did not answer
    And the owner's decisions are beside the map
