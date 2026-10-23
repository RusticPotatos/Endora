Feature: What the team's work came to
  Live, over 17 and 18 October 2026, 201 turns and $8.85 delivered no change the owner
  accepted, 362 posts were written against 46 acts, and nothing on the HQ screen said so:
  every number on it counted activity. The outcomes line counts what the activity bought
  (ADR 0132), from HQ's own record: the changes that landed, what one cost, talk against work,
  and what waits. Nothing it cannot know is guessed.

  # --- A change landed ---------------------------------------------------------------------

  Scenario: A change has landed only once the node pushed what the owner approved
    Given Mara is the CEO
    And Ada is the lead
    And Dee is the tester
    And the goal "Keep the home lab healthy"
    And the engineering wing
    And Kit's planning turn is over
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-tidy-the-deluge-role"
    And the check of "hq/task-2-tidy-the-deluge-role" passes
    Then the outcomes say 0 changes landed this week
    And the outcomes give no cost for a landed change
    And the outcomes say 1 branch submitted and 1 check passed this week
    When Rae submits "Tidy the deluge role" for review
    And Dee passes "Tidy the deluge role" on the check and her own reading
    And Kit accepts "Tidy the deluge role"
    Then the owner's desk holds a push of "hq/task-2-tidy-the-deluge-role"
    And the outcomes say 0 changes landed this week
    When the owner approves the push
    Then the outcomes say 0 changes landed this week
    When the node says it pushed: "Pushed hq/task-2-tidy-the-deluge-role to the fork: merge it on GitHub."
    Then the outcomes say 1 change landed this week

  Scenario: Money spent with nothing landed has no cost for a change
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Look at sonarr"
    When Linus's turns on "Look at sonarr" spend $0.10 and then $0.20
    Then the outcomes say $0.30 was spent this week
    And the outcomes say 0 changes landed this week
    And the outcomes give no cost for a landed change

  # --- Talk against work -------------------------------------------------------------------

  Scenario: Talk against work is counted as the ceremony guard counts it
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the project "Home lab upkeep" led by Ada
    And Ada has the task "Plan the backups" in the project "Home lab upkeep"
    When Ada talks about "Plan the backups" for 12 turns and changes nothing
    And HQ looks at how the team is doing
    Then the outcomes say 12 turns of talk against 0 turns of work this week
    And the guards card says "12 coordination turns, 0 product changes in 24 h"

  # --- What waits --------------------------------------------------------------------------

  Scenario: Turns that wait are counted by what they wait for, with the oldest wait
    Given a pool of $0.50 a day
    And Ada is the lead
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    And 20 minutes pass
    Then 1 turn waits for budget and none for the Mac, the oldest for 20 minutes

  # --- Memory ------------------------------------------------------------------------------

  Scenario: Work sent back because it already existed is counted as reinvented
    Given Ada is the lead
    And Grace is the tester
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Restart the downloads"
    When Linus submits "Restart the downloads" for review
    And Grace sends back "Restart the downloads" because it already exists: "The restart job is in the media stack."
    Then the outcomes count 1 piece of work reinvented this week

  Scenario: Asking the record and retained historical consultations are counted
    Given Mara is the CEO
    And Ada is the lead
    And Zed is the developer
    And the goal "Keep the home lab healthy"
    And Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed says "Capped sonarr's memory at 512M because the NAS was swapping." as the last word
    And Zed's turn ends
    Given Ada has the task "Plan the week"
    When Ada's turn starts
    And Ada asks the record "Why did we cap sonarr's memory?"
    And a retained answered legacy question from Ada to Zed on "Plan the week" exists
    Then the outcomes count 1 ask of the record, 1 consult asked and 1 answered this week
