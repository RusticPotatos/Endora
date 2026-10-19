Feature: The tools fit the turn
  The bridge offers the team Endora's own tools and a tool server's, under the policy Endora
  already has (ADR 0108). Live, a developer thinking on the house's own model on the NAS asked
  the owner to approve a web map search, with coordinates it had made up, "to find the NAS backup
  location", and the request reached the owner's desk. A request that can reach the owner's desk
  is the one thing a small model is kept from (ADR 0131, amended): a tool that asks the owner, or
  would once a stranger's words are read. Every tool of a server the owner has not reviewed asks,
  web and search tools among them. What the owner trusts to run without asking, the reads the Ops
  lead checks the lab with, it is still given. The owner's Mac and Claude are given tools as their
  role and the data tier allow, as they always were.

  Background:
    Given the tool server "brave-search" has the act "brave_place_search", asking the owner first
    And the tool server "brave-search" has the act "brave_web_search", asking the owner first
    And Endora has the read "weather", switched on

  # Seed: drawing the list up for a turn on the NAS's model the same as for the Mac's fails the
  # first two Thens; keeping every tool from it (the first build) fails the last.
  Scenario: A developer on the house's model on the NAS is offered nothing that asks the owner
    When the team's tools are drawn up for a developer thinking on the NAS's model
    Then "brave-search__brave_place_search" is not among them
    And "brave-search__brave_web_search" is not among them
    But "weather" is among them

  # Seed: keeping every bridged tool from the NAS's model fails the first Then.
  Scenario: The Ops lead on the house's model on the NAS keeps its trusted reads
    Given Endora has the read "sonarr_queue", switched on
    And Endora has the read "radarr_queue", switched on
    And Endora has the act "sonarr_remove_and_search", asking the owner first
    And Endora has the act "sonarr_retry_import", switched on
    When the team's tools are drawn up for a lead thinking on the NAS's model
    Then "sonarr_queue" is among them
    And "radarr_queue" is among them
    And "sonarr_retry_import" is among them
    But "sonarr_remove_and_search" is not among them

  # Seed: judging a tool by its stance alone, and not by whether a call of it can end on the desk,
  # fails the last Then.
  Scenario: An act that runs on its own but is given arguments would ask once a stranger's words are read
    Given Endora has the act "sonarr_remove_and_search", switched on, and given an item
    And Endora has the act "sonarr_retry_import", switched on
    When the team's tools are drawn up for a lead thinking on the NAS's model
    Then "sonarr_retry_import" is among them
    But "sonarr_remove_and_search" is not among them

  Scenario: The same developer on the Mac is offered them as its role allows
    When the team's tools are drawn up for a developer thinking on the Mac
    Then "brave-search__brave_place_search" is among them
    And "weather" is among them

  Scenario: A developer on Claude is offered what a cloud model may read
    When the team's tools are drawn up for a developer thinking on Claude
    Then "weather" is among them
    But "brave-search__brave_place_search" is not among them

  # Seed: refusing every tool to the NAS's model, or none, fails one of the two Thens.
  Scenario: A developer on the NAS's model that names a tool that asks the owner anyway is refused
    When a developer thinking on the NAS's model calls "brave-search__brave_place_search" anyway
    Then it is refused: "a turn on the house's own model on the NAS is given only the tools that cannot end on the owner's desk"
    When a developer thinking on the NAS's model calls "weather" anyway
    Then it is not refused
