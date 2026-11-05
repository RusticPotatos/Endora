Feature: An item outside the lab is briefed as outside the lab
  The defect-remediation climb (O8, 5 November) found the briefing of an item whose loop names a
  public repository outside the lab still telling the member how the home lab is built: ansible-nas
  roles and playbooks, which the item's repository is not. Since ADR 0115's amendment of 5 November
  the lab's rule of the owner's values holds in the lab's fork only, so the briefing of an outside
  item says which repository the work is in and what holds there instead: its branch is kept on
  the node and never pushed, and the owner's own values never go into it. An item of the lab
  keeps the lab's paragraph.

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "simpleeval"
    And the loop "simpleeval" has the item "Walk shared containers once", with the criteria
      | Self-referential containers evaluate |

  # Seed: a briefing that tells every item how the lab is built fails the Thens.
  Scenario: The briefing of an outside item names its repository, not the lab
    Given the owner names "https://github.com/example/tiny" as the repository of the loop "simpleeval"
    And the loop hands out its items
    When Bo's turn starts
    Then Bo's briefing says "https://github.com/example/tiny, a public repository outside the lab"
    And Bo's briefing says "Never put the owner's own values"
    And Bo's briefing says "never pushed"
    And Bo's briefing does not say "managed by ansible-nas"
    And Bo's briefing does not say "How the lab is built: read_world"

  # Seed: a briefing that drops the lab for every item fails the Then.
  Scenario: The briefing of a lab item keeps how the lab is built
    Given the loop hands out its items
    When Bo's turn starts
    Then Bo's briefing says "How the lab is built: read_world world:lab"
    And Bo's briefing does not say "outside the lab"
