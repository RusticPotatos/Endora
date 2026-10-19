Feature: A request for the owner's yes says what it serves
  Live, a developer on the house's small model on the NAS asked the owner to approve a web map
  search, with coordinates it had made up, "to find the NAS backup location", and it reached the
  owner's desk as an approval. The tools that model is offered are narrowed to the ones it can
  choose among (ADR 0131, amended); and whoever asks, a request for the owner's yes says which
  criterion of its task it serves, or it is refused to the member with the reason and never
  reaches the desk (ADR 0124, amended).

  Background:
    Given Ada is the lead
    And Linus is the developer
    And Linus works with the Mac
    And the goal "Keep the home lab healthy"

  # Seed: ask_approval taking a request with no `serves` fails the refusal step; one that puts
  # the card on the desk before checking fails the empty-desk step.
  Scenario: A Mac developer's request that says what it serves reaches the desk showing it
    Given Linus has the task "Clear the stuck downloads", done when "The download queue is clear"
    When Linus asks the owner to approve "sonarr_remove_and_search" because "it is stuck importing"
    Then it is refused, saying "which of #1's criteria does asking the owner serve?"
    And the owner's desk is empty
    When Linus asks the owner to approve "sonarr_remove_and_search" because "it is stuck importing", serving "1"
    Then it is not refused
    And the owner's desk holds an approval of "sonarr_remove_and_search", serving "The download queue is clear"

  # Seed: skipping the task's own criteria lets a request through that serves nothing.
  Scenario: A request that serves none of its task's criteria never reaches the desk
    Given Linus has the task "Find the NAS backup location", done when "The backup folder's path is in the thread"
    When Linus asks the owner to approve "brave-search__brave_place_search" because "to find the NAS backup location", serving "the moon base"
    Then it is refused, saying "which of #1's criteria does asking the owner serve?"
    And the owner's desk is empty

  # Seed: taking a request for a task with no criteria, as the answer to "serving 1", fails the
  # refusal step.
  Scenario: A task with no criteria has nothing for a request to serve
    Given Linus has the task "Find the NAS backup location"
    When Linus asks the owner to approve "brave-search__brave_place_search" because "to find the NAS backup location", serving "1"
    Then it is refused, saying "#1 has no criteria"
    And the owner's desk is empty
