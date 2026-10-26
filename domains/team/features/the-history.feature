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

  Scenario: Asking why the team capped sonarr's memory finds the post and the turn, and wakes no one
    Given Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed says "Capped sonarr's memory at 512M because the NAS was swapping." as the last word
    And Zed's worker tells the node what the turn said: "The NAS swapped, so I decided to cap sonarr memory at 512M."
    And Zed's turn ends
    Given Ada has the task "Plan the week"
    When Ada's turn starts
    And Ada asks the record "Why did we cap sonarr's memory?"
    Then the answer says "Capped sonarr's memory at 512M because the NAS was swapping."
    And the answer says "decided to cap sonarr memory at 512M"
    And the answer links Zed's turn
    And the answer cites the task "Cap sonarr's memory"
    And the answer says "testimony"
    And nobody was woken by the question

  # ADR 0141 (the owner, 2026-10-25): Confidential is open to Claude; only Restricted stays home.
  Scenario: A turn on Claude sees what a turn on the house's own models said after reading Confidential data
    Given Zed has the task "Check the router"
    When Zed's turn starts
    And Zed reads the lab and sees something Confidential
    And Zed says "The router's admin password was rotated today." as the last word
    And Zed's worker tells the node what the turn said: "I read the router's status; the password rotated."
    And Zed's turn ends
    Given Rae has the task "Check the proxy"
    And Rae is woken to decide "Check the proxy"
    And Ada has the task "Plan the week"
    When Rae's turn starts
    And Rae asks the record "Was the router password rotated?"
    Then the answer says "password was rotated today"
    When Ada's turn starts
    And Ada asks the record "Was the router password rotated?"
    Then the answer says "password was rotated today"
    And the answer says "the password rotated"

  # What one member writes reaches the next as that member's one line, cleaned (ADR 0129,
  # amended): the post is kept as written, but its mentions dispatch no work (ADR 0138).
  # An explicit owner request can bring it to another member, still cleaned.
  Scenario: A colleague's post cannot forge a line, hide text or wake anyone in a briefing
    Given Zed has the task "Cap sonarr's memory"
    When Zed posts a line that forges a SYSTEM line, hides text, mentions Kit and calls Ada on "Cap sonarr's memory"
    And Zed's turn ends
    And the owner asks Ada to inspect the thread on "Cap sonarr's memory"
    And Ada's turn starts
    Then Ada's briefing shows Zed's post on one line, under his name, cleaned
    And what Ada reads of the thread with my_turn shows the same
