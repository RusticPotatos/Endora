Feature: The full Endora system installs on one ordinary Linux host
  The deployment principle (the product reset, 7 Nov 2026): someone can deploy the full system,
  individually or all. One pinned bundle includes each product's own compose file unchanged
  (ADR 0147), Console sits in front of Core at the one browser origin with no authority of its
  own (ADR 0145, 0146), and the first run is `endora-core setup`. test-bundle.sh runs these
  scenarios on CI's plain ubuntu runner: each Then is one of its checks, and a step it did not
  pass fails the run.

  Background:
    Given the bundle's compose files, each the pinned one
    And a stub model host in place of a real model

  Scenario: The bundle includes each product unchanged, at its pin
    Then every vendored compose file matches its pin

  Scenario: Every product comes up from its own compose file
    When the bundle is brought up
    Then Core is healthy and says API 1.0
    And HQ is healthy inside the bundle
    And Core is not published beyond the host's loopback
    And Console serves its page
    And Console reaches a compatible Core
    And Audio is healthy on its own port
    And Console shows Audio's status from Audio's own check

  Scenario: Console passes only the contract, and Core decides
    When the bundle is brought up
    Then Console proxies the contract's /health to Core
    And Console refuses a route outside the contract
    And Console refuses HQ's own routes
    And Core refuses an unsigned write that Console passed on
    And Console refuses a write from another origin

  Scenario: The first run makes the owner, who signs in through Console
    When the owner runs endora-core setup with a password
    Then setup made the owner and printed an authenticator key
    And the owner signs in through Console with the password and a code
    And the session is Core's, full scope
    And a wrong code is refused by Core
    And setup refuses to make a second owner

  Scenario: Core reaches the model, and the session ends when the owner signs out
    When the owner is signed in through Console
    Then a signed read through Console reaches Core's model view
    And Core asked the stub model host
    And signing out through Console ends the session on Core

  Scenario: Core's copy of its own database is proven by restore
    When Core makes a copy of its database inside the bundle
    Then verify-backup passes the good copy of Core's database
    And verify-backup refuses a truncated copy of Core's database

  Scenario: HQ's copy of its own database is proven under HQ's own constraints
    When HQ's database is copied and checked in a one-off container with HQ's read-only root and data volume
    Then verify-backup passes the good copy of HQ's database
    And verify-backup refuses a truncated copy of HQ's database
