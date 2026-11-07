Feature: The Live Work map
  The owner's decision M3 (7 Nov 2026): one read-only map of one work item, as HQ recorded it:
  the path it took, its state now, who handed it to whom, the evidence for each step, and what
  waits on the owner. It reads only HQ's own records and shows nothing they do not say: a
  failed check is drawn as failed, "completed" needs a verification on record, and a decision
  waiting on the owner comes first.

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Sol    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"
    And the loop "solo arm"
    And the owner sets the loop "solo arm" to be worked by Sol alone

  Scenario: A question waiting on the owner comes first, and leaves once answered
    Given the loop "ansible-nas" has the item "Raise sonarr's memory"
    When the loop hands out its items
    And Bo asks the owner "May sonarr have 2 GB of the NAS's 16?"
    Then the live map of "Raise sonarr's memory" is "waiting"
    And the live map of "Raise sonarr's memory" puts the question "May sonarr have 2 GB" first for the owner
    When the owner answers the question about "Raise sonarr's memory": "Yes, 2 GB"
    Then the live map of "Raise sonarr's memory" holds nothing for the owner
    And the live map of "Raise sonarr's memory" shows the owner's decision on its question

  Scenario: A failed check stays on the path of work that then passed and was verified
    Given the loop "solo arm" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    And a tick passes
    When Sol takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And Sol submits the branch "hq/document-the-media-role", changing "roles/media/README.md"
    And the check of "hq/document-the-media-role" fails: "syntax failed: command not found"
    Then the live map of "Document the media role" shows a failed check
    When Sol asks for "Document the media role" to be checked again
    And the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol sets "Document the media role" ready, citing her reading
    Then the task "Document the media role" is done
    And the live map of "Document the media role" is "completed": "Done, verified against its criteria"
    And the live map of "Document the media role" goes "assigned, submitted, check, check, closed"
    And only Sol takes part in the live map of "Document the media role"
    And the live map of "Document the media role" holds nothing for the owner
