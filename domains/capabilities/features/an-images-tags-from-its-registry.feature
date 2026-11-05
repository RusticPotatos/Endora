Feature: An image's tags from its registry
  HQ failed "Pin the homepage and stats roles off the latest image tag" twice: a sandbox is
  offline, so nothing could show that the release it chose exists (ADR 0106, amended
  2026-10-30). The team's image_tags reads an image's newest tags, and whether a named tag
  exists, from the public registries a lab's images come from: Docker Hub, ghcr.io, quay.io and
  lscr.io, which serves from ghcr.io. Versions come highest first. Any other registry is refused
  before anything is asked.

  Scenario: A member confirms that the release it pins exists
    Given ghcr.io has the image "gethomepage/homepage" with the tags "main, v0.9.9, latest, v1.4.5, v1.10.0, nightly"
    When a member looks up the tags of "ghcr.io/gethomepage/homepage", asking for "v1.4.5"
    Then the answer says "v1.4.5" exists
    And the newest tags are "v1.10.0, v1.4.5, v0.9.9, main, latest, nightly"

  Scenario: A tag that is not there is said not to exist
    Given Docker Hub has the image "grafana/grafana" with the tags "latest, 12.4.14, 12.4, 12.3.9"
    When a member looks up the tags of "grafana/grafana", asking for "12.9.0"
    Then the answer says "12.9.0" does not exist

  Scenario: Only plain releases, when the member asks for them
    Given Docker Hub has the image "grafana/grafana" with the tags "latest, 12.4.14, 12.4, 12.4.14-ubuntu, 12.3.9"
    When a member looks up the plain releases of "grafana/grafana"
    Then the newest tags are "12.4.14, 12.3.9"

  Scenario: An lscr.io image is read on ghcr.io, which serves it
    Given ghcr.io has the image "linuxserver/sonarr" with the tags "4.0.9, 4.0.10, latest"
    When a member looks up the tags of "lscr.io/linuxserver/sonarr", asking for "4.0.10"
    Then the answer says "4.0.10" exists
    And the answer notes "lscr.io serves these images from ghcr.io"

  Scenario: An image on any other registry is refused before anything is asked
    When a member looks up the tags of "registry.example.com/team/app", asking for "1.0.0"
    Then it is refused: "only images on Docker Hub (docker.io), ghcr.io, quay.io and lscr.io can be looked up, by a reference like grafana/grafana, ghcr.io/gethomepage/homepage or lscr.io/linuxserver/sonarr:4.0.0"
    And no registry was asked
