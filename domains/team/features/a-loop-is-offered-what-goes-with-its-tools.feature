Feature: A loop is offered the lookups that go with its tools
  A loop's tools are the owner's list (ADR 0113, amended). In round 2 (31 Oct) the node's
  image_tags shipped, but the ansible-nas loop listed team, shell, repo_checkout and repo_submit,
  so its members never found the lookup and asked the owner which tags exist. A read-only lookup
  that goes with a tool on the list now comes with it: image_tags with repo_checkout. It reads,
  so it gives a read-only task nothing to change the lab with.

  Background:
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the project "ansible-nas" led by Ada with a cap of $20 a month

  Scenario: A loop that checks out the fork may look up an image's tags
    Given the project "ansible-nas" gives its work "team, shell, repo_checkout, repo_submit"
    When Ada is handed the task "Pin the homepage image" in the project "ansible-nas"
    Then Ada's turn starts on "Pin the homepage image"
    And Ada's turn may use the node's "repo_checkout, repo_submit, image_tags"
    And Ada's turn may not use the node's "restart_container"

  Scenario: A read-only task in that loop still may not change the lab
    Given the project "ansible-nas" gives its work "team, repo_checkout, restart_container"
    When Ada is handed the task "Read only: which homepage tags exist" in the project "ansible-nas"
    Then Ada's turn starts on "Read only: which homepage tags exist"
    And Ada's turn may use the node's "repo_checkout, image_tags"
    And Ada's turn is refused the node's "restart_container"

  Scenario: A loop that does not check out the fork is not given the lookup
    Given the project "ansible-nas" gives its work "homelab_status, container_logs"
    When Ada is handed the task "Look round the lab" in the project "ansible-nas"
    Then Ada's turn starts on "Look round the lab"
    And Ada's turn may use the node's "homelab_status, container_logs"
    And Ada's turn may not use the node's "image_tags"
