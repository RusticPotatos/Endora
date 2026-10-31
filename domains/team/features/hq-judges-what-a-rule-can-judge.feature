Feature: HQ judges what a rule can judge
  In the baseline experiment a person re-checked every claim HQ's work made: whether quoted log
  lines were really in the logs, whether a branch changed only allowed files (ADR 0142, amended
  2026-10-30). What a rule can decide, HQ now decides itself when work reaches review, posts on
  the item and tells the reviewer, as facts to weigh. The reviewer still decides, except in one
  case: a branch that touches the owner's own values is sent back by HQ at once, counts as one
  of the item's failures, and wakes no reviewer.

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"

  Scenario: Work reaching review is checked, and the reviewer is told what HQ measured
    Given the loop "ansible-nas" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    And a tick passes
    When Bo takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And Bo submits the branch "hq/document-the-media-role", changing "roles/media/README.md, docs/notes.md"
    And Bo asks for review of "Document the media role", quoting "listening on 8096" and "port 9000 open" from his reading
    Then the thread of "Document the media role" says "HQ's mechanical checks"
    And the thread of "Document the media role" says "1 of 2 quoted lines are in their receipts"
    And the thread of "Document the media role" says "`port 9000 open`: NOT in its result"
    And the thread of "Document the media role" says "Outside the paths the task names (roles/media/README.md): docs/notes.md"
    And Rae is woken to review "Document the media role"
    When Rae's turn starts
    Then Rae's briefing says "1 of 2 quoted lines are in their receipts"
    And Rae's briefing says "These are facts HQ measured"

  Scenario: A branch that touches the owner's own values is sent back at once
    Given the loop "ansible-nas" has the item "Tidy the media role": "Tidy the role's tasks."
    And a tick passes
    When Bo takes "Tidy the media role" and reads media's logs: "media: ready"
    And Bo submits the branch "hq/tidy-the-media-role", changing "roles/media/tasks/main.yml, group_vars/all.yml"
    And Bo asks for review of "Tidy the media role", citing his reading
    Then the task "Tidy the media role" is doing
    And the thread of "Tidy the media role" says "FORBIDDEN (never to be touched): group_vars/all.yml"
    And the thread of "Tidy the media role" says "Sent back by HQ: the branch touches group_vars/all.yml"
    And Bo is woken to redo "Tidy the media role"
    And Rae was not asked to look at "Tidy the media role"
    And "Tidy the media role" has 1 failure
