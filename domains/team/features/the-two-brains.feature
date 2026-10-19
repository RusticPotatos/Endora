Feature: Two brains per member
  Cheap models read and do; the expensive one decides on summaries (ADR 0131). Every member
  has a work brain, the Mac or the house's own model, and may have a judgment brain, a Claude.
  What kind a turn is, and so where it runs, is decided by HQ from why the turn was asked for,
  and never by the member. A turn that cannot run where it is allowed waits, and says why: it
  is never given another brain, and a wake is never thrown away for money.

  Background:
    Given turns have kinds, with a pool of $0.50 a day

  # --- Judgment runs only on Claude --------------------------------------------------------

  Scenario: A decision for a member out of budget waits, and runs after the reset
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And the budget chip counts 1 turn waiting
    And Ada has no turn running
    When a day passes
    Then Ada's turn starts on "Cap sonarr's memory"
    And Ada's turn runs on "claude-sonnet-5-5"
    And the budget chip counts 0 turns waiting

  Scenario: A decision waits for a top-up of the pool
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    When the owner tops up the pool by $0.50
    Then Ada's turn starts on "Cap sonarr's memory"

  Scenario: A decision never runs on the Mac, even when the Mac is free
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada works with the Mac and judges with no one
    And the node says the Mac is ready
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for a judgment brain
    And Ada has no turn running
    And nothing is running on the Mac
    When the owner gives Ada "Claude Sonnet" to judge with
    Then Ada's turn starts on "Cap sonarr's memory"
    And Ada's turn runs on "claude-sonnet-5-5"

  Scenario: A decision is not folded into a work turn that already runs
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    And the node says the Mac is ready
    When Ada is handed the task "Look at sonarr"
    And Ada's turn starts
    Then Ada's turn runs on "mac"
    When Ada is woken to decide "Look at sonarr"
    Then the decision waits for Ada's work turn to end
    When Ada's turn ends
    Then Ada's turn starts on "Look at sonarr"
    And Ada's turn runs on "claude-sonnet-5-5"

  # --- Work runs on the best free work brain it is allowed on ------------------------------

  Scenario: Code work with the Mac busy waits and is never run on the house's model
    Given Mara is the CEO
    And the goal "Keep the home lab healthy"
    And the engineering wing
    And the node says the Mac is not ready: "owner active"
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When a tick passes
    Then Rae's turn on "Tidy the deluge role" is waiting for the Mac: "owner active"
    And Rae has no turn running
    When the node says the Mac is ready
    Then Rae's turn starts on "Tidy the deluge role"
    And Rae's turn runs on "mac"

  Scenario: Code work for a member who works only on the house's model waits for the Mac
    Given Mara is the CEO
    And the goal "Keep the home lab healthy"
    And the engineering wing
    And Rae works with the house's model
    And the node says the Mac is ready
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When a tick passes
    Then Rae's turn on "Tidy the deluge role" is waiting for the Mac: "this work needs the Mac, and the member's work brain is the house's model"
    And Rae has no turn running

  Scenario: Reading the lab with the Mac busy runs on the house's model
    Given Sam is the researcher
    And the goal "Keep the home lab healthy"
    And Sam works with the Mac
    And the node says the Mac is not ready: "owner active"
    When Sam is handed the task "Read the lab's disk use"
    Then Sam's turn starts on "Read the lab's disk use"
    And Sam's turn runs on "local"
    And nothing is running on the Mac

  Scenario: Reading the lab takes the Mac when it is free
    Given Sam is the researcher
    And the goal "Keep the home lab healthy"
    And Sam works with the Mac
    And the node says the Mac is ready
    When Sam is handed the task "Read the lab's disk use"
    Then Sam's turn starts on "Read the lab's disk use"
    And Sam's turn runs on "mac"

  Scenario: Work costs nothing, even for a member who judges with Claude
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    When Ada is handed the task "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"
    And Ada's turn runs on "local"
    And the pool has paid nothing

  # --- Nothing is dropped ------------------------------------------------------------------

  Scenario: A wake out of budget is kept, not refused
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    Then no wake was refused for budget

  Scenario: Wakes for the same decision collapse into one waiting turn
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And Ada is woken to decide "Cap sonarr's memory"
    And Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada has 1 turn waiting for budget
    And the budget chip counts 1 turn waiting

  Scenario: A turn waiting for budget is still there after HQ restarts
    Given HQ keeps its team in a file
    And Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada judges with Claude
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    And HQ restarts
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And the budget chip counts 1 turn waiting
    When a day passes
    Then Ada's turn starts on "Cap sonarr's memory"
