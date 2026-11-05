@mirror
Feature: The team works in a public repository outside the lab, and nothing leaves the node
  Round 4 of the baseline experiment adds F4: one task each round in a public open-source
  repository HQ has never seen, done by both arms against criteria written first. A member's
  sandbox has no internet, and repo_checkout served only the lab's ansible-nas fork. So the owner
  names the repository on the item's project, as a public https://github.com address and nothing
  else; the node clones it once into a mirror of its own and fetches it on the fork's schedule;
  repo_checkout hands its default branch to the sandbox; repo_submit keeps the member's branch in
  that mirror for judging. Delivery never pushes it anywhere, and that is the mirror's own rule.

  # Seed: a mirror that hands out the fork whatever the item names fails the Then.
  Scenario: A checkout of the repository the item names
    Given a public repository "https://github.com/example/tiny" the owner named for an item, whose default branch is "main"
    When a member checks out the outside repository
    Then the checkout holds the repository's "main"
    And it holds no upstream

  # Seed: keeping the submission nowhere, or pushing it to the repository, fails a Then.
  Scenario: A submission is kept on the node for judging
    Given a public repository "https://github.com/example/tiny" the owner named for an item, whose default branch is "main"
    When a member submits "hq/task-936-fix-parse" on top of "main" as HQ's own commit
    Then it is taken
    And the node keeps it as "refs/submissions/hq/task-936-fix-parse"
    And the repository has no branch "hq/task-936-fix-parse"

  # Seed: a submission rule that ignores which repository it is fails the Then (ADR 0115,
  # amended 2026-11-05): the owner's values live in the lab's fork, not in a public repository.
  Scenario: An outside repository's own inventory and vault code are taken
    Given a public repository "https://github.com/example/tiny" the owner named for an item, whose default branch is "main"
    When a member submits "hq/task-936-count-stock" on top of "main" as HQ's own commit, adding "shopkeep/inventory/stock.py, inventory/models.py, app/vaultwarden_client.py, pkg/vaulted_cache.py"
    Then it is taken

  # Seed: a rule that waves every path through fails the Then.
  Scenario Outline: The lab's fork still refuses the owner's values
    Given the owner's fork of ansible-nas, worked from "develop"
    And upstream cannot be reached
    When a member submits "hq/task-936-tidy" on top of "develop" as HQ's own commit, adding "roles/media/tasks/main.yml, <path>"
    Then it is refused, saying "touches <path>, which holds the owner's own values"

    Examples:
      | path                      |
      | inventory/hosts.yml       |
      | inventories/prod/hosts    |
      | group_vars/all.yml        |
      | host_vars/nas.yml         |
      | inventory.yml             |
      | inventory.ini             |
      | vault.yml                 |
      | roles/x/vars/vault.yml    |
      | app/vaultwarden_client.py |

  # Seed: an outside mirror that pushes like the fork's fails both Thens.
  Scenario: Delivery never pushes it anywhere
    Given a public repository "https://github.com/example/tiny" the owner named for an item, whose default branch is "main"
    And a member's branch "hq/task-936-fix-parse" on "main" was taken
    When the node is asked to push it to the repository
    Then it is refused, saying "is never pushed"
    And the repository has no branch "hq/task-936-fix-parse"

  Scenario: Delivery never merges it either
    Given a public repository "https://github.com/example/tiny" the owner named for an item, whose default branch is "main"
    And a member's branch "hq/task-936-fix-parse" on "main" was taken
    When the node is asked to merge it into the repository's "main"
    Then it is refused, saying "is never pushed"

  Scenario: A public GitHub address is kept in one plain form
    When the owner names "https://github.com/example/tiny.git" as an item's repository
    Then it is kept as "https://github.com/example/tiny"

  # Seed: a check that takes any https address fails every example but the first.
  Scenario Outline: Any other address is refused
    When the owner names "<address>" as an item's repository
    Then it is refused, saying "only a public https://github.com/"

    Examples:
      | address                                    |
      | http://github.com/example/tiny             |
      | git@github.com:example/tiny.git            |
      | https://gitlab.com/example/tiny            |
      | https://github.com.example.com/example/tiny |
      | https://github.com:443/example/tiny        |
      | https://github.com/example/tiny/tree/main  |
      | file:///srv/tiny.git                       |
