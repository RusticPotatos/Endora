Feature: A model change never leaves a member unable to start
  Claude Code speaks only Anthropic's API, so it runs a developer who thinks with Claude and
  nobody else (ADR 0117). In round 2 (31 Oct) the owner moved Rae to DeepSeek with Claude Code
  still set, and the runner refused every turn of hers for three hours. Now a model Claude Code
  cannot run puts the member back on Endora's worker, and the owner is told so; and Claude Code
  is refused for a member whose model it cannot run.

  Scenario: A developer on Claude Code moved to DeepSeek runs on the worker, and the owner is told
    Given Rae is the developer
    And Rae thinks with Claude
    And Rae runs on Claude Code
    And the goal "Keep the home lab healthy"
    When the owner sets Rae's model to "deepseek-flash"
    Then the owner is told "Rae now runs on Endora's worker: Claude Code runs only Claude"
    And Rae runs on Endora's worker
    When Rae is handed the task "Pin the homepage image"
    Then Rae's turn starts on "Pin the homepage image"
    And Rae's turn is handed to the runner on Endora's worker

  Scenario: A developer on Claude Code moved to another Claude keeps Claude Code
    Given Rae is the developer
    And Rae thinks with Claude
    And Rae runs on Claude Code
    When the owner sets Rae's model to "claude-haiku-5-5"
    Then the owner is told nothing about Rae's runtime
    And Rae runs on Claude Code

  Scenario: Claude Code is refused for a developer on DeepSeek
    Given Rae is the developer
    And Rae's model is "deepseek-flash"
    When the owner sets Rae's runtime to "claude-code"
    Then it is refused for "Claude Code is for a member on Claude alone"
    And Rae runs on Endora's worker
