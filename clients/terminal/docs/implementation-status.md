# Portable console implementation checkpoint

The portable console is in progress. Version 0.2.1 contains the local interface,
private setup and device-code convenience change. The full acceptance plan is
not complete. No pushes, PRs or public releases were made.

## Implemented

- Seven-page Rust interface with streaming AI, Home catalog/favorites, media,
  NAS and Jobs presentation, confirmations and uncertain-operation reconciliation.
- Origin-bound Argon2id/XChaCha20-Poly1305 privileged vaults, separate dashboard
  sessions, private home/away settings, masked legacy-session migration and logout.
- Four-character minimum, with no default device code. Existing longer codes
  remain supported. Short codes offer weaker protection against offline guessing.
- NetworkManager picker, temporary portal/auth/home browser profiles, namespace
  isolation for locked portals, volatile browser storage and backlight support.
- Pocket/wake handling, Travel/Desk timing, bounded private timing suggestions,
  optical activity/wake and reopening of disconnected input devices.
- Versioned packages, rollback, optional device helpers, and a recovery launcher
  that does not execute arbitrary input or fall through to a shell.
- Endora source: narrow dashboard authorization, fixed-route typed controls,
  state read-back, selected-user media approval behavior, protected containers,
  normalized metrics, bounded logs and a durable mutation journal.
- Host job agent, private catalog wizard and disabled-by-default Ansible role.
  Output compaction preserves deduplication IDs. Jobs remain serialized/durable.

## Validation

Native Rust formatting, Clippy and behavioral tests cover encryption, four-character
codes, real masked CLI migration, private file modes, SSE fragmentation, uncertain
submission handling, pointer behavior, power transitions and bounded learning.
Developer tests cover privacy failures, package allowlists, installation/rollback
and recovery. PTY checks exercise both Linux-console and xterm rendering.
ARM64 builds use remapped source paths and explicit release allowlists.

Endora checks and focused tests cover dashboard scope restrictions, request IDs,
fixed upstream routes/headers, read-back, approval state, privacy projections,
favorite ordering and job-agent persistence/redaction. Synthetic tests establish
contracts; they do not establish real service or field-trial acceptance.

## Remaining work

- Complete actual device enrollment/unlock and physical scrolling acceptance.
  Make code prompts clearer on small Linux consoles, including cursor restoration.
- Finish foreground terminal ownership for supervised shell/SSH/home-browser
  children; audit cleanup, inactivity and crash behavior on the device.
- Verify native SSH Access authentication without leaving persistent CLI token
  caches. Remote locked status must not retain privileged Access credentials.
- Finish integration private setup, selected requesting user, bookmarks, favorites,
  inventory/catalog mapping and the host service installation. Reuse existing
  services and preserve the NAS checkout's prior changes.
- Deploy the final server refinements after checks; distinguish source changes
  from the currently running image in private deployment notes.
- Test repeated real browser handoffs and captive portals, remote session expiry,
  Wi-Fi loss during actions, crash recovery, pocket/wake, rollback and boot disable.
- Measure memory pressure and whole-device power; conduct the twelve-hour field
  trial and separate unplugged-runtime measurement. No runtime promise is made.

Cloudflare application routing requires an owned domain and explicit Access
policies. Without that input, leave remote routing unconfigured; never expose an
anonymous temporary route or silently substitute another network design.

Keep addresses, credentials, actual inventory, device observations and deployment
notes in ignored private files. See portable.md for operation and setup details.
