# One Endora: the handheld, the house and recovery

Agreed with the owner on 2026-10-16 through a design interview (27 questions, every one
settled). Each phase is its own branch off `develop`, merged after the full gate, the
personal-data review and the owner's OK. Prepared with AI assistance (Claude).

## Why

- **The handheld is a separate project.** `endora-terminal` (a Rust TUI for a small handheld)
  talks to Endora over its own endpoints, and imports nothing from it, so a change on one side
  can break the other without either build noticing. It also does things the web console
  cannot (media requests, container controls, jobs, Spotify), while the web console, which is
  what the owner has on their phone, should be the full set.
- **Recovery was partial.** See [ADR 0098](../adr/0098-it-can-be-rebuilt.md).
- **Setup is manual.** Every connection is typed into the console by hand; nothing would come
  back by itself after a rebuild, and someone else installing Endora starts from nothing.
- **What AI it runs is not visible.** Models shows the everyday and deep models; the mail
  reader's model, vision, speech-to-text, context size and where each runs are not shown.

## Decisions

- **One repository.** The handheld moves into this repository as `clients/terminal`, under
  Apache-2.0, with a small shared protocol crate holding the `/v1` wire types both sides
  compile against. The handheld still imports no server internals.
- **The web console is the full set; the handheld is a lighter subset for a lighter device.**
  Anything the server does is one server feature offered to both. Settings that only mean
  something on the handheld (Wi-Fi, Bluetooth, sound, display, passcode, power) stay there,
  shown read-only on a Devices page.
- **One setup, either client.** Endora holds every connection and its credentials. Both
  clients render the same setup flow, so a sign-in started on one can be finished on the other
  and shows its progress on both. Spotify is the first: one shared sign-in.
- **Devices.** A Devices page lists handhelds (online, version, battery); a handheld pairs with
  a short code shown there and can be revoked there. Notifications stay with Home Assistant.
- **ansible-nas is how the house configures Endora, and Endora can run it.** Endora's roles
  ship from this repository as an `endora.homelab` Ansible collection, which the owner's
  ansible-nas pins. The `endora` role renders the declared configuration from the vault and
  fills in the addresses ansible-nas already knows. In the other direction, Endora runs
  allowlisted check and apply jobs through the host job agent, each apply approved by the
  owner; its own roles are excluded.
- **Configuration is declared, memory is backed up** ([ADR 0098](../adr/0098-it-can-be-rebuilt.md)).
  The declared file wins at every start; the console shows those settings as managed and can
  export the current settings to seed the file. A first-run wizard serves people without
  ansible-nas.
- **An AI screen** shows every model role, where it runs, its context size, what data it sees
  and what it costs, all editable.

## Phases

1. **Safety net.** Nightly and weekly consistent copies of the database with a check and a line
   in Activity; the ansible-nas checkout into the house's backup without the vault password;
   the vault password kept in a password manager and encrypted under a recovery passphrase;
   [the runbook](../recovery.md).
2. **AI screen.**
3. **One repository.** Import the handheld; the shared protocol crate; the `endora.homelab`
   collection; fold in the house's uncommitted Endora work, reviewed piece by piece.
4. **Shared connections and parity.** Server-side setup flows (Spotify first); media,
   containers, jobs and Spotify in the web console; Mail, Inbox and Activity on the handheld;
   Devices and pairing.
5. **Declared configuration.** The file from the vault, managed settings, export, the
   first-run wizard, and a restore drill into a scratch node.
6. **ansible-nas from Endora.** The job agent installed; approved check and apply jobs.

Deferred until the owner chooses a provider: an offsite copy of the backups.
