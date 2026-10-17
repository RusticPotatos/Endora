Feature: The owner's devices are never the team's
  The bridge offers the team Endora's own tools, under the policy Endora
  already has (ADR 0108). On 17 October 2026 a developer asked to turn a
  container "off" through the owner's smart-home switches, to restart it.
  The team's remit is the home lab's containers: every act of the tool server
  the owner paired with Home Assistant is kept off the bridge, never listed
  and refused if called. The node draws up a turn's tool list from this same
  rule, and resolves a call against that list, so what is kept off here is
  refused there.

  Background:
    Given the owner paired Home Assistant with the tool server "home-assistant"

  Scenario: A switch is never on the team's tool list and refused if called
    Given that server has the act "HassTurnOff", switched on
    And that server has the read "GetLiveContext", the owner's named reader
    When the team's tools are drawn up
    Then "home-assistant__HassTurnOff" is not among them
    But "home-assistant__GetLiveContext" is among them
    When a member calls "home-assistant.HassTurnOff" anyway
    Then it is refused: "the owner's devices, never the team's"

  Scenario: Only the server the owner paired counts
    Given the owner paired Home Assistant with the tool server "house"
    And the tool server "house" has the act "HassTurnOff", switched on
    And the tool server "home-assistant" has the act "HassTurnOff", switched on
    When the team's tools are drawn up
    Then "house__HassTurnOff" is not among them
    But "home-assistant__HassTurnOff" is among them

  Scenario: Nothing that reaches an address of the member's choosing
    Given Endora has the read "web_fetch", switched on
    When the team's tools are drawn up
    Then "web_fetch" is not among them
    When a member calls "web_fetch" anyway
    Then it is refused: "a way out of the team's fence"
