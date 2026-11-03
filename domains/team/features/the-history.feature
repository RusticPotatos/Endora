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

  # ADR 0129, amended 2026-11-01: the worker tells the conversation after every model round,
  # so the owner can watch a turn as it works; each tell is the whole so far, and replaces the last.
  Scenario: The owner watches a running turn's conversation as it goes
    Given Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed's worker tells the node what the turn said: "I read the logs."
    Then the replay of Zed's turn is live
    And the replay of Zed's turn shows "I read the logs."
    When Zed's worker tells the node what the turn said: "I read the logs.\nI will cap sonarr at 512M."
    Then the replay of Zed's turn shows "I read the logs." once
    And the replay of Zed's turn shows "I will cap sonarr at 512M."
    When Zed's turn ends
    Then the replay of Zed's turn is not live

  # ADR 0129, amended 2026-11-01: the model's reasoning is kept, before what it said and called.
  Scenario: The owner reads what a turn thought before each step
    Given Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed's worker tells the node the turn thought "sonarr swaps, so cap it" and then said "Capping sonarr."
    And Zed's turn ends
    Then the replay of Zed's turn shows "--- thinking ---\nsonarr swaps, so cap it\n--- assistant ---\nCapping sonarr."

  Scenario: The owner sees what a turn was given: its tools by name, its rules and its briefing
    Given Zed has the task "Cap sonarr's memory"
    When Zed's turn starts
    And Zed's worker tells the node the turn started with the tools "post, claim", held back "container_logs", the rules "Look, then act." and the briefing "Your task: cap sonarr."
    And Zed's turn ends
    Then the replay of Zed's turn shows "--- tools ---\npost\nclaim\n"
    And the replay of Zed's turn shows "--- tools: held back ---\ncontainer_logs\n"
    And the replay of Zed's turn shows "--- system ---\nLook, then act."
    And the replay of Zed's turn shows "--- user ---\nYour task: cap sonarr."

  Scenario: A conversation over a megabyte keeps its head and its tail
    Given Zed has the task "Read the logs"
    When Zed's turn starts
    And Zed's worker tells the node a conversation of two megabytes that starts "START" and ends "END"
    And Zed's turn ends
    Then the replay of Zed's turn is cut, and still starts with "START" and ends with "END"

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

  # my_turn shows a thread's newest twelve posts. Older ones were dropped with no sign under
  # "The thread, newest last:", so an owner's early word could fall out of what a member read
  # while the thread read as whole. Seed: without the line saying so, the Then fails.
  Scenario: A thread longer than a turn reads says what it left out, and where it is
    Given Zed has the task "Cap sonarr's memory"
    When the owner posts 13 lines on "Cap sonarr's memory"
    And Zed's turn starts
    Then what Zed reads of the thread with my_turn says "the thread's 1 older post (ask_record with task"
    And what Zed reads of the thread with my_turn says "Owner line 13"
