Feature: What every pass reads first
  Ralph (Huntley; snarktank/ralph) gives every pass of its loop the same prompt, and keeps the
  few patterns its passes learned at the top of a file every pass reads. HQ's turns began with
  who the member is and the day, so no turn could read its opening from the cache another
  turn's wrote; and what one turn learned about a loop reached the next only if it happened to
  read the thread. Every work turn now opens with the same words, and each loop keeps a few
  lessons that its work turns read right after the item (ADR 0142, amended 2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" starts at Bo's rank
    And the loop "ansible-nas" has the item "Find what stops gluetun"
    When the loop hands out its items

  # Seed: a briefing's first line was who the member is, its rank and the day.
  Scenario: Every work turn opens with the same words
    Given the loop "Backups"
    And the loop "Backups" starts at Rae's rank
    And the loop "Backups" has the item "Pin the restic image"
    When the loop hands out its items
    Then Bo on "Find what stops gluetun" and Rae on "Pin the restic image" are told the same opening before who each is

  # Seed: what a turn learned was in the thread, and the thread is cut to its newest posts.
  Scenario: A send-back teaches the loop, and the next turn reads it right after the item
    When Bo asks for review of "Find what stops gluetun", saying "Lesson: gluetun's provider is set in its role's defaults."
    Then the loop "ansible-nas" has no lessons
    When Rae sends "Find what stops gluetun" back, saying "Lesson: every default goes in the role's README with its value."
    Then the loop "ansible-nas" has the lessons:
      | gluetun's provider is set in its role's defaults.      |
      | every default goes in the role's README with its value. |
    And Bo's next turn on "Find what stops gluetun" reads the lessons right after the item

  Scenario: The loop keeps only a short new line with nothing secret in it
    Given the loop "ansible-nas" has the lessons:
      | Pin images in each role's defaults. |
    When Bo asks for review of "Find what stops gluetun", saying "Lesson: pin IMAGES in each role's defaults"
    And Rae approves "Find what stops gluetun", noting lessons a line too long, one holding a token, and "ansible-lint runs from the fork's root."
    Then the loop "ansible-nas" has the lessons:
      | Pin images in each role's defaults.     |
      | ansible-lint runs from the fork's root. |

  Scenario: A full list lets its oldest lesson go
    Given the loop "ansible-nas" has 10 lessons, the oldest "Lesson number 1."
    When Bo asks for review of "Find what stops gluetun", saying "Lesson: the newest."
    And Rae sends "Find what stops gluetun" back, saying "The README is missing."
    Then the loop "ansible-nas" has 10 lessons, the newest "the newest."
    And the loop "ansible-nas" no longer has the lesson "Lesson number 1."

  Scenario: The owner edits the list, and a line with a secret is refused
    When the owner sets the lessons of "ansible-nas" to "Run the lint from the root." and "Lesson: keep roles small."
    Then the loop "ansible-nas" has the lessons:
      | Run the lint from the root. |
      | keep roles small.           |
    When the owner sets the lessons of "ansible-nas" to "the key is sk-abcdefabcdefabcdefabcdef" and "keep it"
    Then it is refused, saying "never holds a secret"
    And the loop "ansible-nas" has the lessons:
      | Run the lint from the root. |
      | keep roles small.           |
