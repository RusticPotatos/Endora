# 0023 — The egress guard: SSRF protection and a data-loss tripwire

## Status

Accepted (2026). Complements
[ADR 0022](0022-autonomy-envelope-and-self-authored-capabilities.md) (the autonomy
envelope) and [ADR 0021](0021-capability-catalog-and-mcp-host.md) (capabilities),
and upholds the local-first, privacy-preserving posture of the constitution.

## Context

Endora is local-first: the model, database, and reasoning run on the person's own
hardware, and their understanding/preferences are fed only to the *local* model.
But some capabilities **reach outside the machine** (weather, news, web search,
knowledge, fetching a web page). Two protections already exist at the **policy**
layer: each skill declares `reaches_external`, and the autonomy envelope
(`auto_external`) can require confirmation before any device-leaving skill runs.

What is missing is a **network** layer that governs *what may leave and to where*:

1. **`web_fetch` is an SSRF hole.** It fetches any http(s) URL the model produces,
   including private/internal addresses (`192.168.x`, `127.0.0.1`, the cloud
   metadata address `169.254.169.254`). A prompt-injection from fetched content
   could steer it at the internal network or exfiltrate via a crafted URL. The same
   applies to any capability that fetches a model/person-provided URL (e.g. an image
   URL for image review).
2. **No egress choke point** — no host allowlist, no outbound content inspection, no
   record of what actually left.

Note a crucial asymmetry: the butler's calls to the **local model** (Ollama) target
an internal address *by design*. A guard must therefore protect **arbitrary
(model/person-provided) URLs**, not the trusted internal service calls.

## Decision

Introduce an **egress guard** the outbound path routes through, built in slices.

### Slice 1 (this ADR): SSRF protection on arbitrary URLs

- A pure guard `guard_egress(url)` that, before fetching a **model/person-provided**
  URL: requires an http(s) scheme; extracts the host; and blocks the request if the
  host is, or **resolves to**, a non-public address — loopback, RFC1918 private,
  link-local (incl. `169.254.169.254`), unspecified, multicast, or IPv6
  unique-local/link-local. Resolving first defeats hostname tricks that point at
  internal IPs.
- **Redirects are followed manually and re-guarded** at each hop (bounded), so a
  public URL cannot 3xx-redirect into an internal one.
- Applied to the URL-taking skills — `web_fetch` and image review's `image_url`
  fetch — via `guarded_get_text` / `guarded_get_bytes`. The hardcoded-API skills
  (weather, news, …) keep their normal fetch to constant, trusted hosts, and the
  internal model calls are untouched.

### Later slices (declared, not built here)

- **Per-capability host allowlist** enforced at one choke point.
- **Outbound content tripwire**: scan skill inputs for obvious secrets (API keys,
  long tokens) and optionally PII before they leave; block or confirm.
- **Egress logging** to the existing action feed (`EventLog`), so the person can see
  exactly what left and where.

### Invariant

The guard is deterministic and sits *below* the capability, so it holds regardless
of what the model asks for — consistent with "models propose, policy authorizes."

## Consequences

- The concrete SSRF hole in `web_fetch` (and image-URL fetch) is closed: the butler
  can't be steered into the internal network or a metadata endpoint, even under
  prompt-injection.
- A small, well-tested guard with no new dependency (manual host parse + std DNS
  resolution + `IpAddr` range checks). Redirect handling becomes explicit.
- A narrow residual risk remains (DNS rebinding between check and connect); noted for
  a later slice that validates the connected socket. The private-IP block already
  makes the common exfiltration paths fail closed.
- Trusted internal calls (the local Ollama) and constant-host API skills are
  deliberately exempt, so the guard adds no friction to normal operation.

## Alternatives considered

- **Block all external skills.** Maximally safe, but guts the useful capabilities
  and duplicates what the autonomy envelope already lets the person choose. Rejected.
- **Trust the model not to fetch internal URLs.** The model is never the enforcement
  boundary (ADR 0005). Rejected.
- **Adopt a URL/SSRF crate.** Reasonable later, but a focused std-only guard closes
  the hole now without a new dependency; revisit if the allowlist/rebinding slices
  need it.
