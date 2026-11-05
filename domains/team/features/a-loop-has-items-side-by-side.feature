Feature: A loop has items side by side
  In the baseline experiment of 28 October eight of the ten tasks sat in one loop, and a loop
  handed out one item at a time, so they ran one after another for hours while other members
  sat idle (ADR 0142, amended 2026-10-30). A loop now has up to two items in hand at once
  unless the owner says, from one to four, and each goes to a different member: the member at
  the loop's starting rank, then the nearest free one above. A paused member, or the Mac's
  while the Mac is away, is passed over; with nobody free, an item waits rather than stacking
  on one member. The Mac is the owner's laptop: while it is away, its member's items go one
  rank up, and the turns it had waiting on them end, so nothing waits on the laptop.

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"

  Scenario: Two items start together on two members, and a third waits its turn
    Given the loop "ansible-nas" has on its plan, in this order
      | Fix the lint    |
      | Pin the images  |
      | Tidy the README |
    When a tick passes
    Then "Fix the lint" is Bo's
    And "Pin the images" is Rae's
    And "Tidy the README" is nobody's yet
    And Bo is woken to take "Fix the lint"
    And Rae is woken to take "Pin the images"
    When the owner drops the task "Fix the lint"
    And a tick passes
    Then "Tidy the README" is Bo's

  Scenario: One item at a time, when the owner says so
    Given the owner sets the loop "ansible-nas" to 1 item at a time
    And the loop "ansible-nas" has on its plan, in this order
      | Fix the lint   |
      | Pin the images |
    When a tick passes
    Then "Fix the lint" is Bo's
    And "Pin the images" is nobody's yet

  Scenario: The owner chooses from one to four items at once
    When the owner sets the loop "ansible-nas" to 0 items at a time
    Then it is refused
    When the owner sets the loop "ansible-nas" to 5 items at a time
    Then it is refused
    When the owner sets the loop "ansible-nas" to 4 items at a time
    Then it is not refused
    Given the loop "ansible-nas" has on its plan, in this order
      | Fix the lint       |
      | Pin the images     |
      | Tidy the README    |
      | Bump the collection |
      | Retire the old role |
    When a tick passes
    Then "Fix the lint" is Bo's
    And "Pin the images" is Rae's
    And "Tidy the README" is Ada's
    And "Bump the collection" is Kit's
    And "Retire the old role" is nobody's yet

  Scenario: An item never stacks on a member who already holds one
    Given the loop "ansible-nas" starts at Mara's rank
    And the loop "ansible-nas" has on its plan, in this order
      | Find what stops gluetun |
      | Pin the images          |
    When a tick passes
    Then "Find what stops gluetun" is Mara's
    And "Pin the images" is nobody's yet

  Scenario: A paused member is passed over for the next one up
    Given the owner pauses Rae
    And the loop "ansible-nas" has on its plan, in this order
      | Fix the lint   |
      | Pin the images |
    When a tick passes
    Then "Fix the lint" is Bo's
    And "Pin the images" is Ada's

  Scenario: The Mac's member is passed over while the Mac is away
    Given the node says the Mac is not ready: "on battery"
    And the loop "ansible-nas" has on its plan, in this order
      | Fix the lint   |
      | Pin the images |
    When a tick passes
    Then "Fix the lint" is Bo's
    And "Pin the images" is Ada's

  Scenario: While the Mac is away its member's item goes one rank up, and its waiting turn ends
    Given the loop "ansible-nas" starts at Rae's rank
    And the loop "ansible-nas" has on its plan, in this order
      | Fix the lint |
    And the node says the Mac is ready
    When a tick passes
    Then "Fix the lint" is Rae's
    When Rae's turn starts
    And Rae's turn ends
    And the owner replies on "Fix the lint": "Only the roles under roles/media, please."
    And the node says the Mac is not ready: "busy"
    And a tick passes
    Then "Fix the lint" is Ada's
    And Ada is woken to take "Fix the lint"
    And Rae has no turn waiting on "Fix the lint"
