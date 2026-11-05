Feature: The owner's values are guarded where they live
  The owner's values live in the lab's fork of ansible-nas: its group_vars, host_vars, inventories
  and vault files. HQ sends back at once any lab branch that touches one (ADR 0115). The rule was
  applied to every repository, so in a public repository outside the lab an ordinary
  `inventory/` package or a `vaultwarden_client.py` was refused as the owner's values: twice in
  the harness confirmation and 4 of 8 harmless paths in the autonomy climb's fixtures. Which
  repository a task is done in is the owner's to say, on its project; the rule follows that, never
  a member's word (ADR 0115, amended 2026-11-05).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo

  # Seed: a guard that ignores the repository fails the first Then.
  Scenario: An outside repository's own inventory and vault code go to review
    Given the loop "shopkeep"
    And the owner names "https://github.com/example/shopkeep" as the repository of the loop "shopkeep"
    And the loop "shopkeep" has the item "Count the stock", with the criteria
      | Stock counts add up |
    And the loop hands out its items
    When Bo submits the branch "hq/task-1-count-the-stock", changing "shopkeep/inventory/stock.py, inventory/models.py, app/vaultwarden_client.py, pkg/vaulted_cache.py"
    And Bo asks for review of "Count the stock"
    Then the thread of "Count the stock" does not say "FORBIDDEN"
    And the task "Count the stock" is in_review

  # Seed: a guard that waves through every path fails the Thens.
  Scenario: In the lab the same rule still sends the owner's values back
    Given the loop "ansible-nas"
    And the loop "ansible-nas" has the item "Tidy the media role", with the criteria
      | The role's tasks are tidy |
    And the loop hands out its items
    When Bo submits the branch "hq/task-1-tidy-the-media-role", changing "roles/media/tasks/main.yml, inventory/hosts.yml, roles/x/vars/vault.yml"
    And Bo asks for review of "Tidy the media role"
    Then the thread of "Tidy the media role" says "FORBIDDEN (never to be touched): inventory/hosts.yml, roles/x/vars/vault.yml"
    And the task "Tidy the media role" is doing
