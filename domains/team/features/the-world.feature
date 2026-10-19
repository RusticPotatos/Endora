Feature: The team's world: stages, stories and situations
  What the team knows lives in HQ's world (ADR 0122): the house page with its dated
  timeline, the lab and team pages HQ derives, and stories with a handle. Each piece
  says how established it is, from an observation to canon, and only the owner makes
  something canon. How far a piece is trusted is read from its dated evidence when it
  is read. Each turn starts from a situation, its briefing keeps to a budget and says
  what it left out, and work is never handed on without a handover note.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Dee is the tester
    And the goal "Keep the home lab healthy"

  Scenario: The house log becomes the house page's timeline, and nothing is lost
    Given the house log has the line "- 15 Oct: The NAS has 16 GB."
    And the house log has the line "- 16 Oct 2026: Sonarr may have 2 GB."
    When HQ starts with the world
    Then the house page's timeline says "The NAS has 16 GB." on 15 Oct
    And the house page's timeline says "Sonarr may have 2 GB." on 16 Oct
    And the house log still says "Sonarr may have 2 GB."
    When HQ starts with the world again
    Then the house page's timeline has 2 lines
    Given Mara has the task "Review the house"
    When Mara notes on the house page "Backups moved to 04:00."
    Then the house page's timeline says "Backups moved to 04:00." on 17 Oct

  Scenario: A lesson the tester noted is a story to retell, and its handle stands in for it
    Given the goal's notes hold the lesson "Closed too soon: the alarm came back two hours later." noted by Dee on 10 Oct
    When HQ starts with the world
    Then the story "closed-too-soon" is marked to retell
    Given Ada has the task "Look over the alarms"
    When Ada's turn starts
    Then Ada's briefing says "Lessons of your area to retell as stories"
    When Ada retells "closed-too-soon": when "An alarm's task looks fixed after a restart.", tempting "Close it at once.", instead "Wait an hour with no new alarm, then close.", because "Task 667 came back two hours later."
    Then the story "closed-too-soon" is no longer marked to retell
    When Ada's turn ends
    Given Dee has the task "Test the deluge restart"
    When Dee's turn starts
    Then Dee's briefing says "Story closed-too-soon: When: An alarm's task looks fixed after a restart."
    When Dee posts "Sent back: closed-too-soon."
    Then the story "closed-too-soon" was cited just now

  Scenario: Only the owner makes something canon, and a demotion says why
    Given Mara has the task "Review the house"
    When Mara writes on the house page "Backups": "Backups run at 03:00." as an insight on her reading
    Given Ada has the task "Look at the backups"
    When Ada writes on the house page "Backups": "Backups run at 02:00." as an insight on her reading
    Then it is refused for "Mara"
    When Ada proposes "world:house#backups" for canon because "seen every night this week"
    Then the owner's desk holds a proposal for canon of "world:house#backups" saying "seen every night this week"
    And "world:house#backups" is an insight
    When the owner approves the proposal for canon
    Then "world:house#backups" is canon
    When the owner demotes "world:house#backups" to insight because "the schedule moved to 04:00"
    Then "world:house#backups" is an insight
    And the house page's timeline says "Demoted from canon to insight by the owner on 17 Oct: the schedule moved to 04:00"
    And the feed records the demotion of "world:house#backups" because "the schedule moved to 04:00"

  Scenario: An insight unconfirmed past its horizon is shown so, and a fresh reading clears it
    Given Ada has the task "Look at the alarms"
    When Ada's turn starts
    And Ada tells the story "closed-too-soon" as an insight on her reading
    And Ada's turn ends
    And 31 days pass
    Then the World shows "closed-too-soon" as "unconfirmed since 17 Oct"
    Given Ada has the task "Look again"
    Then Ada's turn starts on "Look again"
    And Ada's briefing says "[unconfirmed since 17 Oct]"
    When Ada confirms "story:closed-too-soon" on a fresh reading
    Then the World shows "closed-too-soon" as "insight"

  Scenario: A recalled member starts with what changed, and nothing is cut unnamed
    Given Rae is the developer
    When the owner furloughs Rae
    And 7 days pass
    Given Mara has the task "The weekly look"
    When Mara notes on the house page "Sonarr moved to the new disk."
    And the owner recalls Rae
    Given Rae has the task "Back to work"
    When Rae's turn starts
    Then Rae's turn is recorded as "recalled"
    And Rae's briefing opens with "What changed since 17 Oct:"
    And Rae's briefing says "Mara: Sonarr moved to the new disk."
    And Rae's briefing says "You were furloughed from 17 Oct to 24 Oct; read your notes first."
    And nothing was left out of Rae's briefing unnamed

  Scenario: Work is handed on only with a handover note, and the next turn starts from it
    Given Zed is the developer
    And Rae is the developer
    And Zed has the task "Fix the proxy"
    When Zed's turn starts
    And Zed's turn ends
    Given Ada has the task "Plan the week"
    Then Ada's turn starts on "Plan the week"
    When Ada hands "Fix the proxy" to Rae without a handover note
    Then it is refused for "handover note"
    When Ada hands "Fix the proxy" to Rae, saying done "The config is read.", not done "The restart.", next "Restart after 22:00.", watch out "The VPN drops for a minute."
    And Ada's turn ends
    Then Rae's turn starts on "Fix the proxy"
    And Rae's turn is recorded as "handover"
    And Rae's briefing opens with "Handed over by Ada on #"
    And Rae's briefing says "Watch out: The VPN drops for a minute."

  Scenario: The house model's briefing keeps to its budget, and says what it left out
    Given Linus is the developer
    And Linus has the task "Tidy the logs"
    When Linus's turn starts
    And Linus tells 40 long stories
    And Linus's turn ends
    And 8 days pass
    And Linus is woken about "Tidy the logs"
    And Linus's turn starts
    Then Linus's turn is recorded as "after_gap"
    And Linus's briefing keeps to its budget of 3000 tokens
    And Linus's briefing ends with "Read any of them with read_world (your notes with read_notebook)."
    And nothing was left out of Linus's briefing unnamed
