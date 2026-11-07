Feature: One member works a loop alone
  Round 4's piping audit (item E of the baseline experiment; the owner, 1 Nov: "let the agents
  be agents") asks whether HQ's own process, not the models, causes the failures. One arm
  settles it: a loop the owner sets to be worked by one named member alone (ADR 0142, amended
  2026-11-01). Its items go to that member only, one at a time. The member checks out, works,
  checks its own criteria and sets the item ready (in review), and the item is done once HQ's
  checks have passed: the check of its newest head, and the mechanical checks at review. Nobody
  reviews it, a failure never climbs the ranks or hands it to anyone, and a turn that runs out
  of room goes on with the same member. Its ninth failure, three ranks' worth (the team arm's
  ceiling), blocks it with the reason and asks the owner, as the team's top rank does. The turn,
  the tools, the checks and the delivery are the team's own; the briefing says she works alone. The member takes no other loop's work and reviews none of it, so the team's
  arm and the solo arm share nobody.

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

  Scenario: The item goes to the named member, and is done when HQ's checks pass and she sets it ready
    The answer to setting it ready says what the item is now: in run 5316 it said "in_review" of
    an item HQ had already closed.
    Given the loop "solo arm" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    When a tick passes
    Then "Document the media role" is Sol's
    And Sol is woken to take "Document the media role"
    When Sol takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And Sol submits the branch "hq/document-the-media-role", changing "roles/media/README.md"
    And Sol sets "Document the media role" ready, citing her reading
    Then it is refused for "the gate has not passed"
    When the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol asks for review of "Document the media role", citing her reading
    Then the task "Document the media role" is done
    And the thread of "Document the media role" says "HQ's checks passed and Sol set it ready: done, with no review"
    And the answer says "is done, not in_review"
    And "Document the media role" closed as "met_criteria": "it met its criteria"
    And the feed says "Document the media role" is done, verified
    And no member is asked to review "Document the media role"

  Scenario: A document cited as evidence is refused with the receipt that wrote it
    Set 4's turns cited the check's or the submit's document (d249) for their criteria: not
    evidence, and the refusal said only "cite a receipt as r12, a task as task 12", so they tried
    again, some several times, and some cited the task itself. The refusal names the receipt.
    Given the loop "solo arm" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    And a tick passes
    When Sol takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And Sol submits the branch "hq/document-the-media-role", changing "roles/media/README.md"
    And the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol sets "Document the media role" ready, citing the check's document
    Then the refusal names the receipt of the check of "Document the media role" to cite
    And the task "Document the media role" is doing

  Scenario: A receipt cited with a line saying what it shows is read as the receipt
    HQ's record to 3 Nov has 48 calls refused whole for a line written beside a good id in one
    criterion's list ("r4031", "the :ro statement is in the thread"). The line is let go; a
    criterion of words alone is still refused.
    Given the loop "solo arm" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    And a tick passes
    When Sol takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And Sol submits the branch "hq/document-the-media-role", changing "roles/media/README.md"
    And the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol sets "Document the media role" ready, citing the check's receipt with a line saying what it shows
    Then the task "Document the media role" is done

  Scenario: An item with no branch is completed, not verified: it cites evidence, and nothing checked it
    The goal-understanding run (task 1057, run 5316) cited a reading for every criterion and
    answered "not yet known"; with no branch to check and no reviewer, HQ closed it as having met
    its criteria. Closing it stays automatic, but it is kept and said as completed, not verified.
    Given the loop "solo arm" has the item "Settle the VPN restart": "Is the 2-hourly restart still needed?"
    And a tick passes
    When Sol takes "Settle the VPN restart" and reads gluetun's logs: "gluetun: healthy"
    And Sol asks for review of "Settle the VPN restart", citing her reading
    Then the task "Settle the VPN restart" is done
    And "Settle the VPN restart" closed as "completed_unverified": "it was completed, not verified: its evidence was cited, and nothing checked that it settles the item"
    And the thread of "Settle the VPN restart" says "completed, not verified"
    And the feed says "Settle the VPN restart" is done, not verified
    And the thread of "Settle the VPN restart" does not say "HQ's checks passed"
    And the answer says "is done, not in_review"
    And the answer says "completed, not verified"
    And no member is asked to review "Settle the VPN restart"
    And the owner's desk holds nothing about "Settle the VPN restart"

  Scenario: Her briefing says she works alone, and that setting it ready closes it
    Every turn opens with the protocol's line "three failures take the work a rank up", the
    same bytes each turn so the prompt stays cached; on her loop nothing climbs (set 4's
    briefings said both), so what she is told of her loop says the line is not hers.
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol's turn starts
    Then Sol's briefing says "You work this loop alone"
    And Sol's briefing says "closes it, with no review"
    And Sol's briefing says "Three failures do not take it a rank up: after 9 it is blocked"
    And Sol's briefing says "Once HQ's check of your newest submission has passed"

  # Climb 3, round 7 (calibration ST rep 2): a solo member on a routine's item in a repository
  # outside the lab was told to wait for "HQ's check" of its submission, a check that does not
  # exist there; it scheduled a check-back past the episode's end and the item was dropped.
  # Seed: the gated wording told on every solo item fails the Thens.
  Scenario: A routine's item outside the lab is told there is no check to wait for
    Given the owner names "https://github.com/example/tiny" as the repository of the loop "solo arm"
    And the loop "solo arm" has the routine "Look after the environment" for Sol, which fires now
    And a tick passes
    When Sol's turn starts
    Then Sol's briefing says "You work this loop alone"
    And Sol's briefing does not say "Once HQ's check"
    And Sol's briefing says "no check of the lab's applies to it: there is nothing to wait for"
    And Sol's briefing says "submit any changes with repo_submit before setting it ready (set_status in_review)"
    And Sol's briefing says "kept on the node for judging, never pushed or deployed"
    And Sol's briefing says "closes it, with no review"

  # Round 8. Seed: a briefing that makes every outside item submit a branch to close fails the Then.
  Scenario: An outside inspection that needs no change closes without a branch
    Given the owner names "https://github.com/example/tiny" as the repository of the loop "solo arm"
    And the loop "solo arm" has the routine "Look after the environment" for Sol, which fires now
    And a tick passes
    When Sol's turn starts
    Then Sol's briefing says "If no changes were needed, do not create a branch merely to close the item"
    When Sol takes "Look after the environment" and reads media's logs: "media: listening on 8096, ready"
    And Sol sets "Look after the environment" ready, citing her reading
    Then the task "Look after the environment" is done

  # Round 8 control: a non-routine item outside the lab, told the same route, closes as the
  # submission and the gate already behave: the outside submission opens the gate itself.
  Scenario: A non-routine item outside the lab is submitted, set ready and done
    Given the owner names "https://github.com/example/tiny" as the repository of the loop "solo arm"
    And the loop "solo arm" has the item "Walk shared containers once": "Make self-referential containers evaluate."
    And a tick passes
    When Sol's turn starts
    Then Sol's briefing says "no check of the lab's applies to it: there is nothing to wait for"
    And Sol's briefing does not say "Once HQ's check"
    When Sol takes "Walk shared containers once" and reads media's logs: "media: listening on 8096, ready"
    And Sol submits the branch "hq/task-1-walk-shared-containers", changing "simpleeval.py"
    Then the gate of "Walk shared containers once" is open
    When Sol sets "Walk shared containers once" ready, citing her reading
    Then the task "Walk shared containers once" is done

  # Seed: the gated wording on a routine's item in the lab fails the Thens.
  Scenario: A routine's item in the lab is told setting it ready closes it, with no check first
    Given the loop "solo arm" has the routine "Look after the environment" for Sol, which fires now
    And a tick passes
    When Sol's turn starts
    Then Sol's briefing says "You work this loop alone"
    And Sol's briefing does not say "Once HQ's check"
    And Sol's briefing does not say "outside the lab: no check"
    And Sol's briefing says "no gate stands between your work and setting it ready"
    And Sol's briefing says "closes it, with no review"

  Scenario: A CI criterion no push can meet is hers to block, with the reason
    Given the loop "solo arm" has the item "Run the lint rules' tests in CI", with the criteria
      | The workflow runs tests/test_lint_rules.py |
      | A CI run on the branch passes              |
    And a tick passes
    When Sol submits the branch "hq/task-1-ci-lint-rules", whose workflow "ci.yml" runs on a push to "main, develop"
    And the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol sets "Run the lint rules' tests in CI" ready, citing the item for each criterion
    Then it is refused for "no CI run can be had for hq/task-1-ci-lint-rules"
    And it is refused for "block the item with the reason, for the owner to decide"
    And the task "Run the lint rules' tests in CI" is doing

  Scenario: Failed checks stay with her, and climb to nobody
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol takes "Pin the images" and reads media's logs: "media: ready"
    And Sol submits the branch "hq/pin-the-images", changing "roles/media/defaults/main.yml"
    And the check of "hq/pin-the-images" fails: "ansible-lint: 2 new warnings"
    And Sol submits the branch "hq/pin-the-images", changing "roles/media/defaults/main.yml"
    And the check of "hq/pin-the-images" fails: "ansible-lint: 1 new warning"
    And Sol submits the branch "hq/pin-the-images", changing "roles/media/defaults/main.yml"
    And the check of "hq/pin-the-images" fails: "yamllint: 1 new error"
    Then "Pin the images" has 3 failures and is still Sol's
    And Kit is not woken to take "Pin the images"
    And the owner's desk holds nothing about "Pin the images"

  Scenario: A turn that runs out of room goes on with her, however often
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol's turns on "Pin the images" run out of room 12 times, keeping nothing
    Then "Pin the images" has 3 failures and is still Sol's
    And Sol is woken to continue "Pin the images"
    And Kit is not woken to take "Pin the images"

  Scenario: The ninth failure blocks it with the reason, and the owner is asked
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol's turns on "Pin the images" run out of room 36 times, keeping nothing
    Then the task "Pin the images" is blocked
    And the owner's desk holds one question about "Pin the images"
    And the thread of "Pin the images" says "failed 9 times with Sol working this loop alone"
    And Kit is not woken to take "Pin the images"

  Scenario: She takes no other loop's work, and reviews none of it
    Given the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has on its plan, in this order
      | Fix the lint   |
      | Tidy the roles |
    When a tick passes
    Then "Fix the lint" is Ada's
    And "Tidy the roles" is Kit's
    When Ada submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"
    And Sol is not woken to review "Fix the lint"

  # Correction-placement review: the close interlock on a routine's check closed done. Seed: HQ
  # ignoring the sandbox report closes the check over the unsubmitted commit; the Then fails.
  Scenario: A routine's check is not closed done over a commit HQ never took
    Given the loop "solo arm" has the routine "Look after the environment" for Sol, which fires now
    And a tick passes
    And Sol's turn starts
    And Sol took "Look after the environment" and read media's logs: "media: listening on 8096, ready"
    And the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Look after the environment" done, the worker reporting "hq/task-1-look-after" at the unsubmitted head
    Then it is refused, naming "hq/task-1-look-after" at the unsubmitted head
    When Sol sets "Look after the environment" done, the worker reporting no work of the item
    Then it is not refused
    And the task "Look after the environment" is done

  # Control: a disposition is no way past HQ's gate.
  Scenario: Leaving work unsubmitted does not open the gate
    Given the loop "solo arm" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    And a tick passes
    When Sol takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And the node recorded Sol's checkout of the base at the handover head
    And Sol submits the branch "hq/document-the-media-role", changing "roles/media/README.md"
    And Sol sets "Document the media role" ready, leaving "hq/task-1-fix" at the unsubmitted head unsubmitted because "not wanted", the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is refused for "the gate has not passed"
