Feature: The judge is tested before a round
  Round 3 of the baseline experiment (31 Oct): Ada, on Haiku, reviewed task 947 four times and
  approved it rightly each time, and none of her verdicts landed: her turns never had the review
  tool loaded, and each went to set_status and was refused (D19). Judging rightly is not enough;
  the verdict must be recorded and acted on. So before a round the judge is tested on known-good
  and known-bad work, on both counts (the experiment's item D, round 4). The cases are in
  tests/fixtures/judge.json, each with the verdict a right judge gives: here every case is run
  through HQ for what its rules find and for the verdict's delivery; a model judge is tested for
  its correctness by giving it each case and comparing its verdict with the one written there.

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" starts at Kit's rank

  Scenario Outline: The judge's verdict on known <kind> work is the right one, and it lands
    Given the loop "ansible-nas" has the judge's case "<case>"
    When the loop hands out its items
    And Kit submits the judge's case "<case>" and asks for review
    Then HQ's checks on the judge's case "<case>" find what it is known for
    And the reviewer of the judge's case "<case>" is offered the review tool up front, unless HQ sent it back itself
    When the reviewer gives the judge's case "<case>" its right verdict
    Then the verdict on the judge's case "<case>" is recorded and acted on

    Examples:
      | kind | case                           |
      | good | every default in the README    |
      | bad  | a default with the wrong value |
      | bad  | the owner's own values touched |
      | bad  | a CI run no push can give      |
