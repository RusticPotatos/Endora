# Security Policy

Security is a core design goal of Endora: deterministic policy — not a language
model — is the enforcement boundary for consequential actions. We take reports
seriously and appreciate coordinated, responsible disclosure.

## Supported versions

Endora is **pre-1.0** and in a foundation phase. Only the latest `0.x` release
and the active branches receive security attention.

| Version | Supported |
| --- | --- |
| latest `0.1.x` | ✅ security fixes |
| `main` / `develop` | ✅ best-effort; fixes land here first |
| `< 0.1.0` (pre-release) | ❌ not supported |

Until a `1.0` release there is no long-term support window; upgrade to the latest
`0.x` to receive fixes.

## Deployment note (no authentication yet)

Endora's HTTP API is **unauthenticated** in `0.x` — it is designed as a
local-first, single-user service and binds to `127.0.0.1` by default. **Do not
expose the node on an untrusted network.** In particular, the container image
defaults to binding all interfaces (`0.0.0.0`); publish it only on loopback or a
private network (e.g. `-p 127.0.0.1:8787:8787`), behind a reverse proxy that adds
authentication if remote access is needed. Authentication is tracked as pre-1.0
work.

For running the node always-on and reaching it securely from other devices — a
private overlay network (e.g. Tailscale/WireGuard) or an authenticating reverse
proxy — see the [hosting guide](docs/hosting.md).

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities**, and please do
not disclose them publicly before we have had a chance to coordinate a fix.

Report privately using **GitHub Security Advisories**:

1. Go to the repository's **Security** tab → **Report a vulnerability** (GitHub's
   private vulnerability reporting).
2. Include: a description, affected component/version or commit, reproduction
   steps or a proof of concept, impact, and any suggested remediation.

We will acknowledge your report, work with you on assessment and a fix, and
coordinate disclosure timing with you. Please give us reasonable time to remediate
before any public disclosure.

## Scope

Security-relevant issues include, but are not limited to:

- **Permission bypass** — any path that performs a consequential action without
  the required authorization, or that circumvents the policy layer.
- **Model prompt injection that crosses the deterministic boundary** — input that
  causes a model's proposal to be executed without proper deterministic
  authorization. (Models propose; policy authorizes — a proposal being *made* is
  not itself the vulnerability; the boundary being *crossed* is.)
- **Unsafe capability execution** — a capability doing more than policy
  authorized, or ignoring reversibility/proportionality constraints.
- **Data leakage** — exposure of user data to unintended parties, destinations,
  or logs; violations of the local-first / privacy guarantees.
- **Cross-user or cross-context memory access** — reading or writing memory that
  belongs to another user or another bounded context without authorization.
- **Secret exposure** — leaking credentials, tokens, or keys in code, logs,
  errors, or artifacts.
- **Authentication / authorization failures** — flaws in how identity or
  permissions are established or checked.
- **Memory corruption** — memory-safety defects (note: `unsafe` is forbidden in
  the workspace, so these should be rare and are high priority if found).

## Out of scope (generally)

- Vulnerabilities in third-party dependencies with no impact on Endora (please
  still tell us; we may pursue upstream).
- Issues that require a fully compromised host or physical access.
- Missing hardening that has no demonstrated exploit, absent a concrete impact.

When in doubt, report it privately and let us assess.
