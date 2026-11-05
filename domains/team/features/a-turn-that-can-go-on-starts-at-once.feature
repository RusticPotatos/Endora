Feature: A turn that can go on again is taken up at once
  HQ settles a turn the runner says has ended, and starts a turn brought forward by
  the owner's reply, on its tick: every ten seconds on the NAS. In the
  agentic-harness climb's interrupted run on 4 Nov, the owner's reply brought the
  stopped turn forward at once, and then it sat 8.6 seconds for the next tick before
  anything ran. Now the runner's report of an end, and the owner's reply that wakes
  someone, ask HQ for a pass at once. The pass is the ordinary tick, with every guard
  it keeps; how long a failed turn waits before it is tried again is unchanged.

  Background:
    Given Sol is the developer
    And the goal "Ship the watch tracker"
    And Sol has the task "Fix the failing test"
    And Sol's turn starts

  Scenario: The runner's report of an end asks for a pass, and that pass settles the turn
    When the runner reports Sol's turn on "Fix the failing test" killed from outside
    Then HQ has been asked for 2 passes
    When the pass HQ was asked for runs
    Then Sol's turn on "Fix the failing test" waits to try again

  Scenario: The pass does not try the turn again before its time
    When the runner reports Sol's turn on "Fix the failing test" killed from outside
    And the pass HQ was asked for runs
    And a tick passes
    Then Sol's turn on "Fix the failing test" waits to try again

  Scenario: The owner's reply brings the waiting turn forward and asks for a pass that starts it
    When the runner reports Sol's turn on "Fix the failing test" killed from outside
    And the pass HQ was asked for runs
    And the owner replies "Please carry on" on "Fix the failing test"
    Then HQ has been asked for 3 passes
    When the pass HQ was asked for runs
    Then Sol's turn on "Fix the failing test" is running again

  Scenario: A reply while the turn still runs asks for a pass that starts nothing more
    When the owner replies "Please carry on" on "Fix the failing test"
    Then HQ has been asked for 2 passes
    When the pass HQ was asked for runs
    Then Sol has a turn on "Fix the failing test"
    And Sol's turn on "Fix the failing test" is running

  Scenario: A reply on finished work is refused and asks for no pass
    When the owner drops the task "Fix the failing test"
    And the owner replies "Please carry on" on "Fix the failing test"
    Then it is refused
    And HQ has been asked for 1 pass

  Scenario: The pass the reply asked for still keeps the owner's kill switch
    When the runner reports Sol's turn on "Fix the failing test" killed from outside
    And the pass HQ was asked for runs
    And the owner pulls the kill switch
    And the owner replies "Please carry on" on "Fix the failing test"
    And the pass HQ was asked for runs
    Then Sol's turn on "Fix the failing test" is not running
