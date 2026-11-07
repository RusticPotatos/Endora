Feature: The map keeps every control the owner had
  The map observes HQ's record; the engine owns the work. The only controls on HQ are the
  owner's own, each the node's existing route: the kill switch, giving work, and the decisions
  on the desk. Nothing a tab offered was lost when the tabs were retired.

  Background:
    Given HQ has John Doe working on "example task" in "example project"
    And the desk has a question "Should the example task go on?"

  Scenario: The kill switch and Give work are in the bar of every place
    When the owner visits every place
    Then each place has the kill switch and Give work in its bar

  Scenario: A question is answered from the map
    When the owner opens the console
    Then the question "Should the example task go on?" can be answered from the map

  Scenario: A blocked item that asks the owner stands out, and opens its Live Work map
    Given John Doe holds "example task", blocked and asking the owner
    When the owner opens the console
    Then the map marks "example task" as blocked and asking the owner
    And tapping "example task" opens its Live Work map

  Scenario: Only a running turn moves
    Given John Doe's turn is running and was heard from just now
    And Jane Doe is idle
    When the owner opens the console
    Then exactly one line moves on the map

  Scenario: A turn gone silent stands still and says since when
    Given John Doe's turn is running but has gone silent
    When the owner opens the console
    Then no line moves on the map
    And the map says when John Doe's turn was last heard from

  Scenario: The office's chess board keeps its home in the Office
    When the owner opens "Office"
    Then the Office shows the studio and the office's games
