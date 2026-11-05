Feature: An item outside the lab is briefed as outside the lab
  The defect-remediation climb (O8, 5 November) found the briefing of an item whose loop names a
  public repository outside the lab still telling the member how the home lab is built: ansible-nas
  roles and playbooks, which the item's repository is not. Since ADR 0115's amendment of 5 November
  the lab's rule of the owner's values holds in the lab's fork only, so the briefing of an outside
  item says which repository the work is in and what holds there instead: its branch is kept on
  the node and never pushed, and the owner's own values never go into it. An item of the lab
  keeps the lab's paragraph, and a member of the engineering wing keeps the fork's own rules on
  the fork's items, not on another repository's.

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo

  # Seed: a briefing that tells every item how the lab is built fails the Thens.
  Scenario: The briefing of an outside item names its repository, not the lab
    Given the loop "simpleeval"
    And the loop "simpleeval" has the item "Walk shared containers once", with the criteria
      | Self-referential containers evaluate |
    And the owner names "https://github.com/example/tiny" as the repository of the loop "simpleeval"
    And the loop hands out its items
    When Bo's turn starts
    Then Bo's briefing says "https://github.com/example/tiny, a public repository outside the lab"
    And Bo's briefing says "Never put the owner's own values"
    And Bo's briefing says "never pushed"
    And Bo's briefing does not say "managed by ansible-nas"
    And Bo's briefing does not say "How the lab is built: read_world"

  # Seed: a briefing that drops the lab for every item fails the Then.
  Scenario: The briefing of a lab item keeps how the lab is built
    Given the loop "simpleeval"
    And the loop "simpleeval" has the item "Walk shared containers once", with the criteria
      | Self-referential containers evaluate |
    And the loop hands out its items
    When Bo's turn starts
    Then Bo's briefing says "How the lab is built: read_world world:lab"
    And Bo's briefing does not say "outside the lab"

  # Seed: the fork's own rules told by who the member is, whatever the item, fail the Then.
  Scenario: A wing member on an outside item is not told the fork's own rules
    Given the loop "simpleeval" for the goal "Improve the lab", led by Bo
    And the loop "simpleeval" has the item "Walk shared containers once", with the criteria
      | Self-referential containers evaluate |
    And the owner names "https://github.com/example/tiny" as the repository of the loop "simpleeval"
    And the loop hands out its items
    When Bo's turn starts
    Then Bo's briefing says "a public repository outside the lab"
    And Bo's briefing does not say "The fork's own AGENTS.md"

  # Seed: the fork's own rules dropped for every item fail the Then.
  Scenario: A wing member on a fork item keeps the fork's own rules
    Given the loop "ansible-nas" for the goal "Improve the lab", led by Bo
    And the loop "ansible-nas" has the item "Tidy the media roles", with the criteria
      | The media roles pass ansible-lint |
    And the loop hands out its items
    When Bo's turn starts
    Then Bo's briefing says "The fork's own AGENTS.md"
