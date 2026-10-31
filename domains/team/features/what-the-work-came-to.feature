Feature: What the team's work came to
  Live, over 17 and 18 October 2026, 201 turns and $8.85 delivered no change the owner
  accepted, 362 posts were written against 46 acts, and nothing on the HQ screen said so:
  every number on it counted activity. The outcomes line counts what the activity bought
  (ADR 0132), from HQ's own record: the changes that landed, what one cost, talk against work,
  and what waits. Nothing it cannot know is guessed.

  # --- A change landed ---------------------------------------------------------------------

  Scenario: Money spent with nothing landed has no cost for a change
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Look at sonarr"
    When Linus's turns on "Look at sonarr" spend $0.10 and then $0.20
    Then the outcomes say $0.30 was spent this week
    And the outcomes say 0 changes landed this week
    And the outcomes give no cost for a landed change

  # --- What each model cost ----------------------------------------------------------------

  # The owner, 31 October 2026: what each model cost, compared over time. Built by hand from
  # HQ's record for round 2's ten tasks; now the Money card keeps it.

  Scenario: The Money card says what each model cost this week against the week before
    Given Ada is the lead
    And Linus is the developer
    And Linus thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And Linus has the task "Look at sonarr"
    When Linus's turns on "Look at sonarr" spend $0.10 and then $0.20
    And 7 days pass
    And the owner gives Linus "Claude Sonnet" to think with
    And Linus's turns on "Look at sonarr" spend $0.25 and then $0.35
    Then the cost by model puts Claude Sonnet first, then Claude Haiku
    And this week Claude Sonnet ran 2 turns for $0.60, $0.30 a turn, 100% of the spend
    And this week Claude Haiku ran no turns
    And the week before Claude Haiku ran 2 turns for $0.30, $0.15 a turn, 100% of the spend
    And Claude Sonnet's cost went up and Claude Haiku's went down

  Scenario: The house's models are in the cost by model at $0, with their turns
    Given Ada is the lead
    And Zed is hired as a developer on the house's model
    And the goal "Keep the home lab healthy"
    And Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed's turn ends
    Then this week Local ran 1 turn for $0.00, $0.00 a turn

  # --- Talk against work -------------------------------------------------------------------

  # --- What waits --------------------------------------------------------------------------

  Scenario: Turns that wait are counted by what they wait for, with the oldest wait
    Given a pool of $0.50 a day
    And Ada is the lead
    And Ada thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    And 20 minutes pass
    Then 1 turn waits for budget and none for the Mac, the oldest for 20 minutes

  # --- Memory ------------------------------------------------------------------------------

  Scenario: Asking the record and retained historical consultations are counted
    Given HQ keeps its team in a file
    And Mara is the CEO
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
