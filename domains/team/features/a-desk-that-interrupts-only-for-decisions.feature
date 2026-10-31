Feature: A desk that interrupts only for decisions
  Live, over 17 and 18 October 2026, thirty items reached the owner's desk in 38.6 hours, about
  nineteen a day, and six of them were real decisions or approvals: the rest were stuck work, a
  guard's finding or a notice. One task was flagged by the stuck guard four times, and a ceremony
  finding came back three times after the owner marked it resolved. The desk now interrupts only
  for what needs the owner's word (ADR 0132). In the loops a notice is told and never waits
  (ADR 0142), and stuck work and the coordination guards no longer reach the desk at all: a
  stall is one of the item's failures.

  # --- What interrupts, and what waits ---------------------------------------------------------

  Scenario: A notice is told and never waits on the owner
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    When Endora tells the owner "Linus restarted sonarr: it was unhealthy. It is now running."
    Then the digest is empty
