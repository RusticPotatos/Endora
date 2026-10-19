Feature: The tools fit the turn
  The bridge offers the team Endora's own tools and a tool server's, under the policy Endora
  already has (ADR 0108). Live, a developer thinking on the house's own model on the NAS asked
  the owner to approve a web map search, with coordinates it had made up, "to find the NAS backup
  location", and it reached the owner's desk as an approval. A small model is a poor judge of
  which outside tool to reach for, so what the bridge offers depends on where a turn thinks
  (ADR 0131, amended): the model on the NAS is given the lab's contracts and no outside tool; the
  owner's Mac and Claude are given them as their role and the data tier allow, as they always were.

  Background:
    Given the tool server "brave-search" has the act "brave_place_search", asking the owner first
    And Endora has the read "weather", switched on

  # Seed: drawing the list up for a turn on the NAS's model the same as for the Mac's fails the
  # first Then.
  Scenario: A developer on the house's model on the NAS is offered no outside tool
    When the team's tools are drawn up for a developer thinking on the NAS's model
    Then "brave-search__brave_place_search" is not among them
    And "weather" is not among them

  Scenario: The same developer on the Mac is offered them as its role allows
    When the team's tools are drawn up for a developer thinking on the Mac
    Then "brave-search__brave_place_search" is among them
    And "weather" is among them

  Scenario: A developer on Claude is offered what a cloud model may read
    When the team's tools are drawn up for a developer thinking on Claude
    Then "weather" is among them
    But "brave-search__brave_place_search" is not among them

  # Seed: kept_from_the_bridge answering nothing for the NAS's model fails the Then.
  Scenario: A developer on the NAS's model that names a tool anyway is refused
    When a developer thinking on the NAS's model calls "brave-search__brave_place_search" anyway
    Then it is refused: "a turn on the house's own model on the NAS is given no outside tool"
