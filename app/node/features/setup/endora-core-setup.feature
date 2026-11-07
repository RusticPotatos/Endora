Feature: endora-core setup makes the owner on first run
  Core is headless (the product reset, revision 6), so its first run is a command on the
  machine it runs on, not a race to a web page: `endora-core setup` makes the owner with a
  password and an authenticator, which is how Core signs people in today. It provisions
  nothing else: model and provider keys, tokens and connections stay the owner's own steps.

  Scenario: Setup makes the owner, who then signs in with the password and a code
    Given a new node nobody owns
    When the owner runs setup with the password "correct horse battery staple"
    Then setup says the owner is made and prints an authenticator key
    When the node starts for the first time
    Then the owner signs in with "correct horse battery staple" and a code from that key
    And a wrong code with the right password is refused

  Scenario: A node that already has an owner is not set up again
    Given a node whose owner was made with the password "correct horse battery staple"
    When someone runs setup with the password "another long password"
    Then setup refuses, saying "already has an owner"
    And the owner still signs in with "correct horse battery staple" and a code from the first key

  Scenario: A short password makes nothing
    Given a new node nobody owns
    When the owner runs setup with the password "short"
    Then setup refuses, saying "twelve characters"
    And the node still has no owner

  Scenario: Setup provisions nothing but the owner
    Given a new node nobody owns
    When the owner runs setup with the password "correct horse battery staple"
    Then setup changed nothing in the node but its sign-in
