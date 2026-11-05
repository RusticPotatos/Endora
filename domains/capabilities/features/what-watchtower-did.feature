Feature: What Watchtower did to the lab
  On 31 Oct sonarr restarted at 03:24 and nobody at HQ could tell why (D11): the NAS's
  Watchtower recreates containers on new images every night at 03:00. homelab_status now says,
  for each container last recreated on a new image, when, and one line says what was updated
  in the last day (ADR 0106, amended 2026-10-31). Endora sees it from Docker alone, through the
  observer it already reads: a container created again on a different image was updated; one
  restarted, or created again on the same image, was not. Nothing of Watchtower's own log is
  read. Endora's own containers are released by Endora, never by Watchtower, and are left out.

  Background:
    Given the owner's clock is 4 hours behind UTC
    And Watchtower runs in the lab
    And at 2026-10-30 22:00 Endora sees "sonarr" on image "a1", "nzbget" on image "b1" and "radarr" on image "c1"

  Scenario: What Watchtower updated overnight is said in one line
    When at 2026-10-31 03:24 "sonarr" is created again on image "a2"
    And at 2026-10-31 03:24 "nzbget" is created again on image "b2"
    And a member reads homelab_status at 2026-10-31 08:00
    Then it says "Updated last night by Watchtower: sonarr, nzbget (03:24)"
    And "sonarr" is said to be updated "31 Oct 03:24"
    And "radarr" is not said to be updated

  Scenario: A restart is not an update
    When at 2026-10-31 03:24 "sonarr" restarts
    And a member reads homelab_status at 2026-10-31 08:00
    Then it says "No image updates seen since 30 Oct 22:00"
    And "sonarr" is not said to be updated

  Scenario: A container created again on the same image was not updated
    When at 2026-10-31 03:24 "sonarr" is created again on image "a1"
    And a member reads homelab_status at 2026-10-31 08:00
    Then it says "No image updates seen since 30 Oct 22:00"
    And "sonarr" is not said to be updated

  Scenario: Updates on two days are each said with their day
    When at 2026-10-30 23:10 "radarr" is created again on image "c2"
    And at 2026-10-31 03:24 "sonarr" is created again on image "a2"
    And at 2026-10-31 14:05 "nzbget" is created again on image "b2"
    And a member reads homelab_status at 2026-10-31 15:00
    Then it says "Updated by Watchtower: last night radarr, sonarr (23:10 to 03:24); today nzbget (14:05)"

  Scenario: Without Watchtower, a new image is still said
    Given Watchtower does not run in the lab
    When at 2026-10-31 03:24 "sonarr" is created again on image "a2"
    And a member reads homelab_status at 2026-10-31 08:00
    Then it says "Updated last night on a new image: sonarr (03:24)"

  Scenario: An update more than a day old leaves the line but stays on its container
    When at 2026-10-31 03:24 "sonarr" is created again on image "a2"
    And a member reads homelab_status at 2026-11-01 09:00
    Then it says "No image updates in the last day"
    And "sonarr" is said to be updated "31 Oct 03:24"

  Scenario: Endora's own containers are released by Endora, not updated by Watchtower
    Given at 2026-10-30 22:00 Endora sees "endora-hq" on image "e1"
    When at 2026-10-31 03:24 "endora-hq" is created again on image "e2"
    And a member reads homelab_status at 2026-10-31 08:00
    Then it says "No image updates seen since 30 Oct 22:00"
    And "endora-hq" is not said to be updated
