Feature: Two brains per member
  Cheap models read and do; the expensive one decides on summaries (ADR 0131). Every member
  has a work brain, the Mac or the house's own model, and a judgment brain, a Claude: Haiku
  until the owner says otherwise. What kind a turn is, and so where it runs, is decided by HQ
  from why the turn was asked for, and never by the member. A turn that cannot run where it is
  allowed waits, and says why: it is never given another brain, and a wake is never thrown
  away for money. The one exception is an alarm, which acts at once.

  Background:
    Given a pool of $0.50 a day

  # --- Everyone has two brains -------------------------------------------------------------

  Scenario: A reviewer on the house's model gets its review on Haiku, not left waiting
    Given Dee is hired as a tester on the house's model
    And the goal "Keep the home lab healthy"
    Then Dee works with "Local" and judges with "Claude Haiku"
    When Dee is asked to review "Tidy the deluge role"
    Then Dee's turn starts on "Tidy the deluge role"
    And Dee's turn runs on "claude-haiku-5-5"

  Scenario: The owner chooses each brain, and the other stays as it was
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    Then Ada works with "Mac" and judges with "Claude Haiku"
    When the owner gives Ada "Claude Sonnet" to judge with
    And the owner sets Ada's brain to "local"
    Then Ada works with "Local" and judges with "Claude Sonnet"

  # --- Judgment runs only on Claude --------------------------------------------------------

  Scenario: A decision for a member out of budget waits, and runs after the reset
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And the budget chip counts 1 turn waiting
    And Ada has no turn running
    When a day passes
    Then Ada's turn starts on "Cap sonarr's memory"
    And Ada's turn runs on "claude-haiku-5-5"
    And the budget chip counts 0 turns waiting

  Scenario: A decision waits for a top-up of the pool
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    When the owner tops up the pool by $0.50
    Then Ada's turn starts on "Cap sonarr's memory"

  Scenario: A decision never runs on the Mac, even when the Mac is free
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And Ada works with the Mac
    And the node says the Mac is ready
    When Ada is woken to decide "Cap sonarr's memory"
    Then Ada's turn starts on "Cap sonarr's memory"
    And Ada's turn runs on "claude-haiku-5-5"
    And nothing is running on the Mac

  Scenario: A decision is not folded into a work turn that already runs
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the node says the Mac is ready
    When Ada is handed the task "Look at sonarr"
    And Ada's turn starts
    Then Ada's turn runs on "mac"
    When Ada is woken to decide "Look at sonarr"
    Then the decision waits for Ada's work turn to end
    When Ada's turn ends
    Then Ada's turn starts on "Look at sonarr"
    And Ada's turn runs on "claude-haiku-5-5"

  # --- Work runs on the best free work brain it is allowed on ------------------------------

  Scenario: Code work with the Mac busy waits and is never run on the house's model
    Given Mara is the CEO
    And the goal "Keep the home lab healthy"
    And the engineering wing
    And Kit's planning turn is over
    And the node says the Mac is not ready: "owner active"
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When a tick passes
    Then Rae's turn on "Tidy the deluge role" is waiting for the Mac: "owner active"
    And Rae has no turn running
    When the node says the Mac is ready
    Then Rae's turn starts on "Tidy the deluge role"
    And Rae's turn runs on "mac"

  Scenario: The wing's lead plans on the Mac or not at all
    Given Mara is the CEO
    And the goal "Keep the home lab healthy"
    And the engineering wing
    And the node says the Mac is not ready: "owner active"
    When a tick passes
    Then Kit's turn on "Plan the clean-up: read the fork's docs and propose the first three branches" is waiting for the Mac: "owner active"
    And Kit has no turn running
    When the node says the Mac is ready
    Then Kit's turn starts on "Plan the clean-up: read the fork's docs and propose the first three branches"
    And Kit's turn runs on "mac"

  Scenario: Another team's lead reads the lab on the house's model with the Mac busy
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the node says the Mac is not ready: "owner active"
    When Ada is handed the task "Read the lab's disk use"
    Then Ada's turn starts on "Read the lab's disk use"
    And Ada's turn runs on "local"

  Scenario: Code work for a member who works on the house's model runs on the free Mac
    Given Mara is the CEO
    And the goal "Keep the home lab healthy"
    And the engineering wing
    And Kit's planning turn is over
    And Rae works with the house's model
    And the node says the Mac is ready
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When a tick passes
    Then Rae's turn starts on "Tidy the deluge role"
    And Rae's turn runs on "mac"

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

  Scenario: Work costs nothing, though the member judges with Claude
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the node says the Mac is ready
    When Ada is handed the task "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"
    And Ada's turn runs on "mac"
    And Ada's turn reserved nothing
    And the pool has paid nothing

  # --- An alarm acts at once ---------------------------------------------------------------

  Scenario: An alarm with the money gone runs at once on the Mac if it is free, and the owner is told
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    And the node says the Mac is ready
    When an alarm "Disk almost full" goes off for Ada
    Then Ada's turn starts on "Disk almost full"
    And Ada's turn runs on "mac"
    And Ada's turn reserved nothing
    And the owner's desk has a notice saying "Ada's alarm turn on"
    And the owner's desk has a notice saying "ran at once on the owner's Mac"

  Scenario: An alarm with the money gone runs at once on the NAS if the Mac is not free
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    And the node says the Mac is not ready: "owner active"
    When an alarm "Disk almost full" goes off for Ada
    Then Ada's turn starts on "Disk almost full"
    And Ada's turn runs on "local"
    And Ada's turn reserved nothing
    And the owner's desk has a notice saying "ran at once on the house's own model on the NAS"

  Scenario: Only an alarm may; a decision with the money gone still waits
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When an alarm "Disk almost full" goes off for Ada
    And Ada's turn starts
    And Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And Ada has 1 turn waiting for budget

  Scenario: An alarm with the money left is judged on Claude like any decision
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    When an alarm "Disk almost full" goes off for Ada
    Then Ada's turn starts on "Disk almost full"
    And Ada's turn runs on "claude-haiku-5-5"
    And the owner's desk has 0 notices saying "alarm turn"

  # --- A turn that waits is a wake, not a stall --------------------------------------------

  Scenario: Work whose turn waits for budget is held, not stuck, and nothing climbs
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Linus's decisions
    When Linus is handed the task "Clear the stuck downloads"
    And Linus's turn starts
    And Linus takes "Clear the stuck downloads"
    And Linus's turn ends
    And Linus is woken to decide "Clear the stuck downloads"
    And a tick passes
    Then Linus's turn on "Clear the stuck downloads" is waiting for budget
    When the team is left alone for 6 hours
    Then the work on "Clear the stuck downloads" is held, not stuck
    And nobody was nudged, sent to decide or climbed to about "Clear the stuck downloads"
    And the owner's desk holds no stuck work
    And the task "Clear the stuck downloads" is doing

  Scenario: Work whose turn waits for the Mac is held, not stuck, and the CEO is never woken
    Given Mara is the CEO
    And Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    When Linus is handed the task "Clear the stuck downloads"
    And Linus's turn starts
    And Linus takes "Clear the stuck downloads"
    And Linus's turn ends
    And the node says the Mac is not ready: "owner active"
    And Linus is sent back "Clear the stuck downloads"
    And a tick passes
    Then Linus's turn on "Clear the stuck downloads" is waiting for the Mac: "owner active"
    When the team is left alone for 6 hours
    Then the work on "Clear the stuck downloads" is held, not stuck
    And nobody was nudged, sent to decide or climbed to about "Clear the stuck downloads"
    And Mara has never been woken
    And the owner's desk holds no stuck work

  # --- Nothing is dropped ------------------------------------------------------------------

  Scenario: A wake out of budget is kept, not refused
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    Then no wake was refused for budget

  Scenario: Wakes for the same decision collapse into one waiting turn
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
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
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    And HQ restarts
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And the budget chip counts 1 turn waiting
    When a day passes
    Then Ada's turn starts on "Cap sonarr's memory"
