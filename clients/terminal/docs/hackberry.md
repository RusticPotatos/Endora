# Q20 device preparation and acceptance

Supported initial baseline: HACKBerryPi Q20, Pi Zero 2 W, Debian 12 ARM64,
720×720 framebuffer. Application layout uses terminal dimensions, not pixels.

## Existing working device

Preserve the OS image and working display/keyboard setup. The verified device uses
the `vc4-kms-dpi-hyperpixel4sq` overlay and kernel USB HID support for the Q20
keyboard. The installer never replaces drivers or boot firmware settings.
NetworkManager owns Wi-Fi; SSH remains managed by the OS.

## Fresh installation

1. Install the same Debian 12 ARM64 baseline and establish SSH and Wi-Fi privately.
2. Verify `/boot/firmware/overlays/vc4-kms-dpi-hyperpixel4sq.dtbo` exists. If missing,
   stop and install the matching vendor-supported firmware; do not use an unrelated
   overlay or run an unreviewed remote setup script.
3. Save a private copy of the firmware configuration, then explicitly enable
   `dtoverlay=vc4-kms-dpi-hyperpixel4sq` in `/boot/firmware/config.txt` if absent.
   Keep the existing VC4 KMS configuration. Reboot and validate the 720×720 console.
4. Verify the Q20 input appears under `/dev/input/by-id` and test every required key.
5. Install the application, complete setup, and run `doctor` before enabling boot.

That hardware preparation is separate from application installation. Display
rotation, console font and keyboard mapping must be tested physically; an SSH
terminal's dimensions do not establish the built-in display's readability.

## Acceptance on the actual device

- Open chat, resize an SSH terminal, stream a reply, scroll history and inspect actions.
- Verify Docker status and Sonarr/Radarr queues after server setup.
- Confirm shell handoff and Wi-Fi settings return to the interface.
- Disconnect/reconnect Wi-Fi locally; offline UI remains usable and never resends chat.
- Enable boot only after manual validation; reboot and check screen/keyboard physically.
- Open a fresh SSH session after reboot. Ensure SSH login does not start the interface.
- Test boot disable and binary rollback before treating the device as an appliance.
- Measure idle RSS and CPU with `ps`; targets are <64 MiB RSS and <2% of one core.

Do not save live diagnostic output or photographs into Git. Boot backups are
private and contain real device configuration. If something fails, use SSH to
run `~/.local/bin/endora-terminal boot disable`, then reboot. Firmware snapshots
are for manual recovery and are never blindly restored over later user changes.
