Feature: A reply whose turn-ending call is not its last runs nothing
  A model may put several calls in one reply. A question to the owner (ask_owner), or moving the
  turn's own task on (set_status to in_review, blocked or done), ends the turn, and the worker
  stopped at the first one, so a question that came after a closing status change never reached
  the owner and the model was never told (the minimum-viable-autonomy climb, round 2, task
  #1144: "account jdoe is LOCKED ... unlock it or leave it locked?" was lost behind
  set_status). Now a reply whose turn-ending call is not its only one and its last runs none
  of its calls: each is refused with why, and the model can send them again, one turn-ending
  call last. Nothing is reordered and nothing is run on the model's behalf.

  Background:
    Given a turn in the workbench

  Scenario: A question after a closing status change is not lost
    When the model replies "set_status 7 in_review, ask_owner", then "ask_owner"
    Then HQ was called with "ask_owner"
    And the model was told that nothing in its first reply ran
    And the turn ended asking the owner

  Scenario: A closing status change after a question runs nothing either
    When the model replies "ask_owner, set_status 7 in_review", then "ask_owner"
    Then HQ was called with "ask_owner"
    And the model was told that nothing in its first reply ran

  Scenario: Two closing status changes in one reply run nothing
    When the model replies "set_status 7 done, set_status 7 blocked", then "set_status 7 done"
    Then HQ was called with "set_status 7 done"
    And the model was told that nothing in its first reply ran
    And the turn ended moving its task on

  Scenario: A refused reply has no effect in the shell
    When the model replies "shell touch made.txt, set_status 7 in_review, ask_owner", then "ask_owner"
    Then the workbench holds no file "made.txt"
    And HQ was called with "ask_owner"

  Scenario: Ordinary calls in one reply all run
    When the model replies "post, post"
    Then HQ was called with "post, post"

  Scenario: Reads before one closing call last all run
    When the model replies "post, set_status 7 in_review"
    Then HQ was called with "post, set_status 7 in_review"
    And the turn ended moving its task on

  Scenario: Starting work does not end the turn
    When the model replies "set_status 7 doing, ask_owner"
    Then HQ was called with "set_status 7 doing, ask_owner"
    And the turn ended asking the owner

  Scenario: Moving another task on does not end the turn
    When the model replies "set_status 8 done, ask_owner"
    Then HQ was called with "set_status 8 done, ask_owner"
    And the turn ended asking the owner

  Scenario: A question HQ refuses can be asked again
    Given HQ refuses the first question to the owner as not evidence
    When the model replies "ask_owner", then "ask_owner r12"
    Then HQ was called with "ask_owner, ask_owner"
    And the turn ended asking the owner

  # Seed: a refusal that only says "one call, last" fails the Thens. In the defect remediation's
  # live runs (5 Nov) a model told only that sent set_status alone, meaning to ask next; the
  # turn ended and the question was never sent.
  Scenario: The refusal says the turn ends when the call runs, and that the owner's decision goes first
    When the model replies "set_status 7 in_review, ask_owner", then "ask_owner"
    Then the model was told "ends it the moment it runs" after its first reply
    And the model was told "if the owner must decide, send ask_owner now" after its first reply
    And the model was told "(set_status, ask_owner)" after its first reply
    And the turn ended asking the owner

  # Seed: a refusal that counted no round, or ran a call once refused, fails a Then.
  Scenario: A model that keeps replying with both runs nothing and ends out of rounds
    When the model replies "set_status 7 in_review, ask_owner" every round
    Then HQ was called with "post"
    And the turn ran out of rounds

  # Seed: a preflight refusal remembered by the repeat guard fails the first Then.
  Scenario: After the worker's own refusal the same question, unchanged, is sent
    When the model replies "ask_owner, set_status 7 in_review", then "ask_owner"
    Then HQ was called with "ask_owner"
    And the turn ended asking the owner

  # Seed: a guard that sent an unchanged question HQ had just refused fails the first Then.
  Scenario: After HQ refuses a question, the same question unchanged is not sent again, and a corrected one is
    Given HQ refuses the first question to the owner as not evidence
    When the model replies "ask_owner", then "ask_owner", then "ask_owner r12"
    Then HQ was called with "ask_owner, ask_owner"
    And the model was told "You already have this result" after its second reply
    And the turn ended asking the owner

