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
