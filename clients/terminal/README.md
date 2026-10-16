# endora-terminal

A Rust terminal appliance for Endora on a HACKBerryPi Q20. The portable console
adds Home, Endora AI, Controls (Home Assistant), TV & Movies, Music, NAS, Jobs,
Bluetooth and Settings pages. Endora owns models,
integrations, authorization and persistence; the Pi owns input and presentation.
The 0.2 series is under implementation; see [validation status](docs/implementation-status.md)
before installing it as an appliance.

## Install on the Pi

Use Debian 12 ARM64 with a working Linux console. Release archives target
`aarch64-unknown-linux-musl`; no Rust toolchain is needed on the device. Releases
are not published automatically. Until a release is published, transfer the
locally built archive over SSH.

```sh
# Obtain the archive and SHA256SUMS from the same trusted release.
sha256sum -c SHA256SUMS
tar -xzf endora-terminal-VERSION-aarch64-linux-musl.tar.gz
sh install.sh
~/.local/bin/endora-terminal setup
~/.local/bin/endora-terminal doctor
~/.local/bin/endora-terminal
```

The setup wizard asks for the node URL and an optional CA PEM file. HTTPS verifies
certificates. The short command `endora` and an Endora application-menu launcher
are installed too. Blank setup fields preserve an existing node and CA.

For a self-signed node, copy its `tls-cert.pem` through a trusted
server connection and provide that local path. Do not copy its private key.
HTTP is allowed only after explicitly choosing a private trusted network; HTTP
does not encrypt passwords, tokens or chat.

Sign in with your Endora password and authenticator code. They are entered without
echo and are not saved. A separate device passphrase encrypts the privileged
session using Argon2id and XChaCha20-Poly1305. It is never stored. A separate
server-enforced dashboard credential permits only aggregate status while locked.
Existing installations migrate with `endora lock-setup` after updating the node;
the legacy plaintext session is deleted only after encrypted setup succeeds.
Sessions expire according to the node's policy (currently 30 days); sign in again
through Device → setup. Initial enrollment happens in Endora's existing web console.

## Run and navigate

```text
endora-terminal                 open the interface
endora-terminal setup           configure connection and sign in
endora-terminal lock-setup      encrypt an existing local session
endora-terminal remote-setup    configure a private Access application profile
endora-terminal bookmarks       manage private browser shortcuts after unlock
endora-terminal wifi            NetworkManager picker
endora-terminal doctor          public-safe diagnostics
endora-terminal logout          delete this device's session file
endora-terminal health          one-shot health check
endora-terminal boot enable     enable console appliance startup
endora-terminal boot disable    restore recorded boot settings
```

Esc (or Enter on an empty chat line) opens the menu. Every letter is the first letter of what
it opens: `H` Home, `E` Endora AI, `C` Controls (Home Assistant), `T` TV & Movies, `M` Music,
`N` NAS, `J` Jobs and `S` Settings, then the shortcuts `B` Bluetooth, `W` Wi-Fi, `L` Lock and
`P` Pocket (screen off). Up/Down and Enter work too, and Esc goes back one level everywhere; a
chat draft is kept. Function keys are unnecessary.
Enter with text submits chat; arrows edit input. Up/Down and PageUp/PageDown
scroll. The wheel scrolls three lines per notch; reaching the bottom follows new
replies again. On the Q20 console, move the optical mouse control up/down to
scroll. The client reads only vertical pointer movement and wheel events from
that mouse device using existing input permissions; sideways movement is ignored.
Terminal emulators use terminal mouse events.
Ctrl-Q exits to appliance recovery. Action metadata appears as
short text entries rather than raw JSON; conversation text remains intact.
The Message box contains only your draft. Longer drafts scroll horizontally
to keep the cursor visible; navigation hints and connection status sit below it.
Home uses `C` to browse the entity catalog and Space to confirm favorite changes.
Media: type a title and press Enter to search, Up/Down to pick a result, then Enter
and Y to confirm the request (Ctrl-R also works). With an empty box it lists recent
requests by title. NAS offers selected
container controls and `I` for bounded logs. Jobs uses Enter to confirm submission,
`I` for the selected job's latest output and `V` to reconcile an uncertain action.
Settings is laid out like a phone's: `W` Wi-Fi (choose a network, Wi-Fi login pages, nmtui),
`B` Bluetooth, `S` Sound, `D` Display (text size, screen-off timing), `N` Network (home/away
profile), `A` Accounts (Endora and Spotify sign-in, passcode, remote profile, browser shortcuts),
`T` Tools (shell, SSH, text browser, bookmarks, desktop), `I` Info (diagnostics) and `P` Power
(restart, shut down, exit to recovery). Exit a child shell to return to Endora.
Menu shortcuts accept uppercase too; uppercase chat text remains uppercase.

Reads reconnect with bounded backoff. A failed chat is never automatically resent;
refresh history before resending an uncertain submission. Replies and action
disclosures are text, never executable instructions. Display memory is bounded;
conversation history is stored only by the server.

The portable console requires a node with dashboard-scoped sessions and explicit
terminal APIs. Older clients and existing server chat APIs remain compatible;
do not install the new locked appliance against an older node. Docker/Sonarr/Radarr
credentials are configured on the server with `endora-node homelab-setup`; see
Endora's `docs/homelab.md`. The Pi never receives those keys. The first dashboard
shows up to 100 cached items per service, 20 per page, alongside actual totals.

## Boot, update, and recovery

Read [device setup](docs/hackberry.md) before enabling boot. Validate the screen,
keyboard, manual client, Wi-Fi and a second SSH connection first. Then run
`endora-terminal boot enable` and reboot when ready. Startup uses tty1 automatic
login as your existing non-root user. The managed launcher replaces the login
shell and offers restart after a crash. Privileged shell access requires device
unlock. This application boundary does not replace OS authentication or encrypt
the entire SD card.
SSH still uses the existing login and never launches Endora automatically.

Boot setup privately records previous getty/profile files, boot target, display
manager state, and firmware files. It disables graphical startup without removing
packages. It does not modify Wi-Fi profiles, SSH settings, firewall or drivers.
Use `endora-terminal boot disable` over SSH to restore original startup.
The launcher waits for an explicit restart after failure and never evaluates
recovery input as shell commands. Run `boot enable` again when upgrading an
existing managed appliance so its login profile uses the recovery launcher.

To update, verify and unpack a new versioned release and run its `install.sh`.
Configuration is preserved; activation is atomic and the previous binary remains.
To roll back: `sh ~/.local/share/endora-terminal/current/install.sh rollback`.
Updates do not reboot or interrupt the running process automatically.
To uninstall, run `sh ~/.local/share/endora-terminal/current/uninstall.sh` after logging
out. It restores boot and removes application releases, retaining configuration.

## Privacy

Actual configuration lives in `~/.config/endora-terminal/config.toml`; sessions
and boot backups live in `~/.local/state/endora-terminal/`. Application-private
directories are `0700`, private files `0600`. Symlinked private paths and unsafe
ownership/permissions are rejected. Tokens are bound to the configured node.
Home and away vaults are separate. Locking (`L` in the menu) aborts network tasks and clears owned
privileged credentials; there is no idle relock.
No automatic chat log or diagnostic dump is written. Logout removes the local
session; it does not revoke the corresponding server session.

Never commit live addresses, SSIDs, keys, personal names, screenshots or backups.
Put one private value per line in ignored `privacy.local` to extend local checks.
The privacy checker scans tracked and non-ignored untracked files and reports
categories, not matched values. Review diffs and run secret scanning before
publishing; automated checks cannot recognize every private value.

## Development and packaging

Rust 1.87+, Python 3.9+ for developer checks, and no Pi-side compilation:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
python3 -m unittest discover -s tests -p 'test_*.py' -v
python3 scripts/check-private.py
git diff --check
```

Cross-build using Rustup's target and Zig 0.13 with cargo-zigbuild 0.20.1:

```sh
rustup target add aarch64-unknown-linux-musl
sh scripts/build-arm64.sh
sh scripts/package.sh
```

Use the Rustup toolchain consistently; a separate system Rust installation may
not have the target. Packaging copies an explicit allowlist into `dist/`, which
is ignored. Build paths, archive owners, timestamps and extended metadata are
removed. CI validates source and produces build artifacts without publishing
a release. Dependencies serve terminal rendering, asynchronous networking,
serialization, unique request IDs, masked entry, filesystem safety, and text width;
the application imports no Endora server crates.

Portable operation, browser requirements, power modes and private server setup
are described in [portable operation](docs/portable.md).

Since 2026-10-16 the terminal is part of the Endora repository (`clients/terminal`), built
and checked with it, under the same Apache-2.0 licence ([ADR 0099](../../docs/adr/0099-the-handheld-is-part-of-endora.md)).
It was written with AI assistance (OpenAI Codex) in a repository of its own that was never
published, and is continued with AI assistance (Claude).
