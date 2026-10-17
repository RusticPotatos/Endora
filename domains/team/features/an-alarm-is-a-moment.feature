Feature: An alarm is a moment, not the state now
  The home lab's alarms reach the team through its routine's one open task
  (ADR 0107). On 17 October 2026 a VPN container restarted, three containers
  behind it stopped for a minute and came back on their own, and the team
  spent an hour on what had already passed, in two duplicate tasks. These
  scenarios are what the owner decided that day, so it stays decided.

  Background:
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And the home lab has the containers "deluge, nzbget, prowlarr, sonarr, owntone"

  Scenario: One task a problem
    Given Ada has the task "Check the home lab"
    And Ada hands Linus the task "Diagnose and fix exited deluge, nzbget, prowlarr" under "Check the home lab"
    When Ada hands Linus the task "Confirm state and restart deluge, nzbget, prowlarr" under "Diagnose and fix exited deluge, nzbget, prowlarr"
    Then it is refused: "#2 already covers this (deluge, nzbget, prowlarr): add to it with post in #2, rather than a second task"
    And the team still has 2 tasks

  Scenario: Another container is another problem
    Given Ada has the task "Check the home lab"
    And Ada hands Linus the task "Diagnose and fix exited deluge, nzbget, prowlarr" under "Check the home lab"
    When Ada hands Linus the task "Look at sonarr" under "Check the home lab"
    Then it is not refused
    And the team has 3 tasks

  Scenario: Coming back is news
    Given the routine "Check the home lab" for Ada, which takes the home lab's alarms
    And Endora raised the alarm "deluge became exited at 13:42"
    When Endora says "deluge is back up at 13:43"
    Then "endora: Recovered: deluge is back up at 13:43" is the last post in the task "Check the home lab"
    And Ada is woken about it
    And the team still has 1 task

  Scenario: A recovery hands out no work
    Given the routine "Check the home lab" for Ada, which takes the home lab's alarms
    When Endora says "deluge is back up at 13:43"
    Then the team has no task

  Scenario: A member is told its tools are its own
    Given Ada has the task "Check the home lab"
    When Ada's turn starts
    Then Ada's briefing says "Your tools at HQ:"
    And Ada's briefing says "Endora's own tools on the home lab (homelab_status and the like) come beside them: what your tool list offers, you have."
