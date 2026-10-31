Feature: What wakes an item in the loops
  Round 2 of the baseline experiment (31 Oct) found three ways an item was woken wrongly. Rae's
  turns failed because the runner could not start her runtime, and her items then sat with
  nobody woken until the owner wrote on each thread (D2). Item 938, blocked and judged, woke Kit
  about every eleven minutes, about $0.39 for nothing (D3). And a blocked item kept asking the
  owner the same thing (D10). A turn that never started is tried again later and is nobody's
  failure; a blocked item waits for the owner's words or the owner's move; and an item has one
  question on the owner's desk at a time (ADR 0142, amended 2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"

  # Seed: with the failed start left as the item's last word, nobody is woken again.
  Scenario: A turn that never started is tried again later, and is not a failure
    Given Bo's turns cannot start, because the runner cannot start his runtime
    And the loop "ansible-nas" has the item "Pin the homepage image"
    When the loop hands out its items
    And every try of Bo's turn on "Pin the homepage image" fails to start
    Then "Pin the homepage image" has no failures and is still Bo's
    When 3 minutes pass
    Then Bo has been woken on "Pin the homepage image" 1 time
    When the runner can start Bo's turns again
    And 10 minutes pass
    Then Bo has been woken on "Pin the homepage image" 2 times
    And Bo's turn starts on "Pin the homepage image"

  # Seed: a blocked item was stranded work, nudged and then failed again and again.
  Scenario: A blocked item wakes nobody while it waits for the owner
    Given the loop "ansible-nas" starts at Kit's rank
    And the loop "ansible-nas" has the item "Judge the gluetun timer"
    When the loop hands out its items
    And Kit blocks "Judge the gluetun timer", saying "the logs do not reach back to the window"
    And Kit's turn ends
    Then the owner's desk holds one question about "Judge the gluetun timer"
    When 3 hours pass, with HQ ticking every few minutes
    Then Kit has been woken on "Judge the gluetun timer" 1 time
    And "Judge the gluetun timer" has no failures and is still Kit's

  # Seed: seeing a question with no words woke whoever asked it.
  Scenario: Seeing a blocked item's question with no words wakes nobody
    Given the loop "ansible-nas" starts at Kit's rank
    And the loop "ansible-nas" has the item "Judge the gluetun timer"
    When the loop hands out its items
    And Kit blocks "Judge the gluetun timer", saying "the logs do not reach back to the window"
    And Kit's turn ends
    And the owner marks the question about "Judge the gluetun timer" seen, with no words
    Then Kit has been woken on "Judge the gluetun timer" 1 time

  Scenario: The owner's answer in words wakes the blocked item's holder
    Given the loop "ansible-nas" starts at Kit's rank
    And the loop "ansible-nas" has the item "Judge the gluetun timer"
    When the loop hands out its items
    And Kit blocks "Judge the gluetun timer", saying "the logs do not reach back to the window"
    And Kit's turn ends
    And the owner answers the question about "Judge the gluetun timer": "Keep the timer, and close this"
    Then Kit is woken about it

  # Seed: a second member's question on the same item went on the desk as a second card.
  Scenario: A second question on an item is refused while the first waits
    Given the loop "ansible-nas" has the item "Raise sonarr's memory"
    When the loop hands out its items
    And Bo asks the owner "May sonarr have 2 GB of the NAS's 16?"
    And the owner pauses Bo
    And Rae asks the owner "May sonarr have 3 GB of the NAS's 16?"
    Then it is refused, saying "already waits on the owner's desk"
    And the owner's desk holds one question about "Raise sonarr's memory"

  # Seed: the answer woke whoever asked, though the item had gone on to another member.
  Scenario: The owner's answer reaches whoever holds the item now
    Given the loop "ansible-nas" has the item "Raise sonarr's memory"
    When the loop hands out its items
    And Bo asks the owner "May sonarr have 2 GB of the NAS's 16?"
    And the owner pauses Bo
    And the owner answers the question about "Raise sonarr's memory": "Yes, 2 GB"
    Then Rae is woken about it
