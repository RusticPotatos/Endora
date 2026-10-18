Feature: The team's history
  HQ keeps what every turn said, scrubbed and capped, for thirty days and the last five
  of a member on a task (ADR 0129). The owner reads it in the turn's replay, between its calls.
  After that only the handover notes and the last words remain.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Zed is the developer
    And Rae is the developer
    And the goal "Keep the home lab healthy"

  Scenario: The owner replays a turn and reads what the model said
    Given Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed's worker tells the node what the turn said: "I read the logs and will cap sonarr at 512M."
    And Zed's turn ends
    Then the replay of Zed's turn shows "I read the logs and will cap sonarr at 512M."
    And the replay of Zed's turn says it is kept for 30 days

  Scenario: A conversation over a megabyte keeps its head and its tail
    Given Zed has the task "Read the logs"
    When Zed's turn starts
    And Zed's worker tells the node a conversation of two megabytes that starts "START" and ends "END"
    And Zed's turn ends
    Then the replay of Zed's turn is cut, and still starts with "START" and ends with "END"

  Scenario: After thirty days only the handover notes and the last words remain
    Given Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed's worker tells the node what the turn said: "I will cap sonarr at 512M."
    And Zed says "Capped sonarr at 512M." as the last word
    And Zed's turn ends
    Given Ada has the task "Plan the week"
    Then Ada's turn starts on "Plan the week"
    When Ada hands "Cap sonarr's memory" to Rae, saying done "The cap is written.", not done "The restart.", next "Restart after 22:00.", watch out "The VPN drops for a minute."
    And Ada's turn ends
    Then the replay of Zed's turn shows "I will cap sonarr at 512M."
    When Rae's turn starts
    And Rae's turn ends
    And 31 days pass
    Then the replay of Zed's turn shows no conversation
    And "Capped sonarr at 512M." is still in the thread of "Cap sonarr's memory"
    And the handover note on "Cap sonarr's memory" still says "Restart after 22:00."
