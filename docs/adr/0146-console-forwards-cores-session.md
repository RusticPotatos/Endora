# 0146 - Console forwards Core's session; no shared sign-in crate (AU1)

## Status

Accepted, 2026-11-07, as the evidence settled it at P1 step 2 (the product reset left AU1's crate
question "open by evidence"). AI-assisted (Claude). Builds on [0145](0145-core-console-contract.md)
and [0125](0125-secrets-at-rest-in-the-node.md).

## Context

AU1 (the product reset, section 16) decided that each product has a built-in sign-in and
secret store, written once as a library, and principle 8 sized it: keep it Core's module, and
make it a small, separately versioned crate only when a second product needs it. Whether a
separate Endora Console is that second product was left to this step.

The question is whether Console needs **a second sign-in implementation**, or only carries
Core's session.

## Evidence

From Console as built (`RusticPotatos/endora-console`, first release) and Core at this commit:

- **Console has no sign-in code.** Its server is under 400 lines of Node with no dependencies:
  a static file server and an allowlist proxy. It has no password hashing, no authenticator
  codes, no session table and no cookie. Its sign-in screens are Core's own screens, copied, and
  they post to Core's `POST /v1/session`.
- **The session passes through untouched.** Console's tests show the browser's
  `Authorization: Bearer` reaches Core exactly as sent, that Console adds none of its own (a
  request without one reaches Core without one), and that cookies and forwarding headers are
  dropped.
- **Core decides every time.** Against real Core binaries (`scripts/e2e-against-core.mjs` in
  Console): claiming and signing in with the password and an authenticator code through Console
  works; a wrong code is refused by Core; a write without a session through Console is `401`
  from Core; the same session works against Core directly with Console out of the way; signing
  out through Console ends the session on Core at once.
- **One origin keeps one session.** Console is served at the browser origin people already use,
  so the session token in the browser's storage is the same one; nothing is re-registered.
  The node has no passkeys today (removed with OpenBao, ADR 0144), so there is no origin-bound
  credential to move.
- **No other product signs people in.** Endora Audio uses one bearer token made on first start
  (fit for a home LAN), and Endora Home has no UI.

## Decision

**Console forwards Core's session; it does not implement sign-in.** AU1 stays Core's module
(`app/node/src/{auth,signin,totp}.rs`, `api/auth.rs`, and the session routes in
`api/console.rs`). **No crate is extracted**: it would be a versioned artifact with one consumer,
which principle 8 rules out, and it would add a coordinated release for nothing.

The trigger to extract it, small and pinned per product: a second product that signs **people**
in (Audio with per-person sign-in, or a Home UI). Not a second client of Core's sessions: the
chat connector (reset section 15) forwards a Core credential the same way Console does.

## Consequences

- Sign-in, expiry and revocation change in one place, Core, and Console needs no release for
  them.
- Console being in the trust path is handled by what it does not hold (0145), not by a sign-in
  of its own.
- If passkeys return, they are registered on the one origin Console serves, and Core verifies
  them; Console still only carries the request.

## Alternatives considered

- **Extract `endora-auth` now and build Console's server on it:** gives Console a sign-in it
  does not need, and two places a session could be judged.
- **Console keeps its own session and calls Core with a service credential:** puts authority in
  Console, which 0145 and the reset rule out (Console holds no authority).
