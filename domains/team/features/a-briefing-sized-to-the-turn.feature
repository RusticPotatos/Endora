Feature: A briefing sized to the turn
  Measured the week of 23 to 30 October, work turns read 110,000 to 213,000 prompt tokens, and
  each call resends the briefing (ADR 0142, amended 2026-10-30). A work turn's briefing carried
  the whole world whatever its item was about; a reviewer's carried the same, and the item's
  whole thread besides. Now a work turn is told the world its item names and where the rest is;
  a reviewer is told what the review needs: the criteria, the branch's diff, HQ's checks and the
  worker's handover, not the thread's history or the world.

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"
    And the house knows "gluetun": "gluetun is the VPN; qbittorrent runs in its network and stops when it restarts."
    And the house knows "media": "jellyfin serves films and series on port 8096."
    And the house knows "dns": "pihole answers names for the house; unbound resolves upstream."
    And the house knows "photos": "immich keeps the house's photos; its database is postgres."

  Scenario: A work turn is told the world its item names, and where the rest is
    Given the loop "ansible-nas" has the item "Find what stops gluetun", done when "gluetun is running and healthy"
    When a tick passes
    Then Bo's turn starts on "Find what stops gluetun"
    And Bo's briefing says "gluetun is the VPN"
    But Bo's briefing does not say "jellyfin serves films"
    And Bo's briefing does not say "immich keeps the house's photos"
    And Bo's briefing says "read_world world:house"

  Scenario: A reviewer is told the criteria, the diff, HQ's checks and the handover, and not the thread
    Given the loop "ansible-nas" has the item "Find what stops gluetun", done when "gluetun is running and healthy"
    And a tick passes
    When Bo takes "Find what stops gluetun" and reads gluetun's logs: "AUTH_FAILED from the provider"
    And Bo posts "Reading the provider's logs again after the restart."
    And Bo submits the branch "hq/find-what-stops-gluetun", changing "roles/gluetun/defaults/main.yml", with the diff
      """
      --- a/roles/gluetun/defaults/main.yml
      +++ b/roles/gluetun/defaults/main.yml
      -gluetun_provider: old
      +gluetun_provider: example
      """
    And Bo asks for review of "Find what stops gluetun", citing his reading
    Then Rae is woken to review "Find what stops gluetun"
    When Rae's turn starts
    Then Rae's briefing says "1. gluetun is running and healthy"
    And Rae's briefing says "Bo asks for your review."
    And Rae's briefing says "HQ's mechanical checks"
    And Rae's briefing says "Files it changes: roles/gluetun/defaults/main.yml"
    And Rae's briefing says "+gluetun_provider: example"
    But Rae's briefing does not say "Reading the provider's logs again"
    And Rae's briefing does not say "gluetun is the VPN"
    And Rae's briefing says "my_turn gives its newest"
