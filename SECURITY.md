# Security Policy

Security is a core design goal of Endora: deterministic policy — not a language
model — is the enforcement boundary for consequential actions. We take reports
seriously and appreciate coordinated, responsible disclosure.

## Supported versions

Endora is **pre-release** and in a foundation phase. There are no stable releases
yet, and version `0.0.x` carries no security guarantees.

| Version | Supported |
| --- | --- |
| `main` / `develop` (unreleased) | Best-effort; fixes land on active branches |
| `0.0.x` (pre-release) | Not supported for production use |

Until a `1.0` release, only the latest state of the active branches receives
security attention. This table will be replaced with a concrete support window
once releases exist.

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
