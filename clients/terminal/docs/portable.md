# Portable operation

Install only after the accompanying Endora node supports dashboard-only sessions
and the terminal endpoints. Run `endora lock-setup` for an existing local session,
or `endora setup` for a new sign-in. Enter the device passphrase locally.
Forgetting it requires sign-in again; there is no passphrase recovery key.
Four characters are accepted for handheld convenience. A four-digit code has
only 10,000 possibilities; a longer phrase better protects a copied SD card.
Entry is hidden, including the repeat prompt; type the code and press Enter.

## Getting around

Esc opens the menu; Esc always goes back one level. Each letter is the first letter of what it
opens, and Up/Down plus Enter work wherever a letter does:

- Menu: `H` Home, `E` Endora AI, `C` Controls, `T` TV & Movies, `M` Music, `N` NAS, `J` Jobs,
  `S` Settings; shortcuts `B` Bluetooth, `W` Wi-Fi, `L` Lock, `P` Pocket (screen off).
- Settings: `W` Wi-Fi, `B` Bluetooth, `S` Sound, `D` Display, `N` Network, `A` Accounts, `T` Tools,
  `I` Info, `P` Power. Inside a section, each action has its own letter.
- Settings → Network shows the Wi-Fi network and signal, IP address, MAC address, gateway, DNS
  servers and VPN state, read again when the section opens and with `R`. `C` connects and `D`
  disconnects the VPN by hand; automatic switching then pauses for 15 minutes so the choice sticks
  (the screen counts it down). `T` tests the VPN: it asks Cloudflare's trace page
  (`https://www.cloudflare.com/cdn-cgi/trace`) whether traffic goes through WARP and which public
  address it sees, and checks that the Endora node answers. `H` switches the home/away profile.
- After unlocking you land on the start screen, set in Settings → Display → `S` (it cycles):
  Home, the menu, or the last page you used.

## Device support

Use the distribution's current Chromium, Xorg/xinit, Openbox, Bubblewrap and
NetworkManager packages. The normal application installer does not install OS
packages, change hardware drivers or configure Wi-Fi.

After reviewing the bundled script, run `sh device-install.sh` explicitly. It
installs a fixed root-owned backlight helper, narrowly scoped sudo rules and a
NetworkManager connectivity probe to Debian's public connectivity-test endpoint.
It reloads configuration without restarting networking or changing profiles.
Uninstallation removes the managed helper and probe. Existing unrelated files
are preserved. Devices without a brightness range support screen off/on only.

Settings → Wi-Fi → `C` (from the menu: `W`, then `C`) scans and joins networks through `nmcli --ask`; passwords stay with
NetworkManager. N opens nmtui for recovery. P opens the portal browser; Ctrl-Q
returns. Manual portal login works even when detection is unavailable. Do not
automatically accept terms or bypass browser certificate warnings. Use a phone
hotspot for incompatible portals.

The graphical session is temporary. The terminal remains alive and polling is
paused while it runs. Locked portal browsing requires Bubblewrap namespaces;
it fails closed if isolation cannot start. The browser sees its temporary
profile and display cookie, system runtime and network configuration, but no
host home directory, SSH keys, session files or removable media. Chromium's
own sandbox stays enabled. Authenticated home shortcuts use a separate temporary
profile and require device unlock. All temporary profiles are removed on return.
Graphical operation and memory pressure must be tested on the actual device.

## Home and away profiles

The home node and optional local CA remain in private config.toml. Away settings,
SSH alias and bookmarks live in private portable.toml. `endora remote-setup`
accepts an Access-protected HTTPS origin and an existing SSH configuration alias.
`endora profile home` and `endora profile away` select explicitly; credentials
are never copied between them.

The selected Cloudflare design requires a domain on the user's account and
separately configured Access application policies. Cloudflare Tunnel uses an
outbound connection and requires no opened inbound router ports. Preserve the
existing tunnel and private routes. Configure origin certificate trust; never
disable TLS verification. Without a domain, leave this path unconfigured.
Full-network WARP is outside this appliance's supported design.

Away setup runs `cloudflared access login`, captures its application token without
displaying it, then signs in to Endora. Both privileged tokens are sealed into
the away vault. Access expiry requires an explicit sign-in; an interrupted chat
or mutation is never automatically resent. Locked away views currently cannot
pass an Access policy requiring the privileged token; unlock to reconnect.
No Access bypass is installed for the dashboard credential.

Use an existing private SSH alias with `ProxyCommand cloudflared access ssh
--hostname ...` and normal strict host-key verification. Cloudflare must already
protect that SSH application. No SSH passwords or private keys are imported by
the application. Browser bookmarks are explicit private URLs configured through
`endora bookmarks` or Settings → Accounts → `B`; they never become model instructions.

## Controls and recovery

Home, media, containers and jobs require unlock and exact-target confirmation.
Service keys and job catalogs live on Endora/the NAS. Container controls cannot
stop the recognized Endora, Cloudflare, Portainer or observer path. Advanced
maintenance uses unlocked SSH. Named Ansible jobs accept no arbitrary commands,
inventory paths or extra arguments from the handheld.

An uncertain operation is saved as a private request identifier. V checks its
record after reconnection. Submission completion does not imply a download or
deployment has finished; follow progress in its page. Server-side jobs continue
when the screen is off or connectivity is lost.

On the home screen each group is a grid, and every service shows its own key and, where
the node has them, its numbers (for example `Radarr  194 have, 28 missing, 6 queued`). Arrows or
the optical control (up/down and sideways) move a cursor across and between rows; Enter, or pressing a service's
key, opens its container log (Esc returns). The passcode is asked once, in a centered box, when the app starts. The device then
stays unlocked until you choose Lock (`L`) in the menu, exit, or change sign-in/profile/encryption.
Pocket and wake do not lock. Privileged foreground children are supervised;
their physical input monitor records no keys or text. Locked exit/crash recovery
offers restart and SSH recovery, never an unrestricted login shell.

## Power and measurements

Travel mode dims after 60 seconds and turns the screen off after three minutes.
Desk uses ten-minute dim and thirty-minute screen-off defaults. P in the menu
enters pocket mode; the first keyboard or optical input wakes without acting.
Wi-Fi remains associated and dashboard polling pauses. This is screen-off mode,
not kernel suspend. No charging state or battery percentage is synthesized.

Local timing history records at most 32 idle durations, with no SSIDs, locations
or conversation content. After eight observations, a suggestion may appear in
Settings → Display. `A` applies it; `R` resets history. Controls are never reordered.

Record real portal use, remote expiry, Wi-Fi loss during actions, repeated browser
returns, crash recovery and pocket/wake. Measure whole-device power, browser
memory pressure, active-use time and charging separately. A twelve-hour day or
unplugged runtime is not established by terminal-only CPU/RSS measurements.

## Power history and battery

The app records one small sample a minute to a private, bounded file
(`~/.local/state/endora-terminal/power-history.jsonl`, rotated at 4 MiB): load, memory
use, temperature, undervoltage, screen stage (awake, dim, pocket), mode and the current
page. It holds no conversation text, network names or locations. Run
`endora-terminal power report` for time and load per screen stage, drain per stage when a
battery level exists, and suggestions.

The HACKBerryPi's power-bank chip (IP5306) exposes no battery level to Linux, so the home
top bar shows no battery at all until a battery gauge is attached. When the kernel provides a
`power_supply` battery, the home screen shows its percentage and a time-left estimate from
the recent drain rate (never while charging, and only with at least 30 minutes of falling
history). Nothing is estimated without a real reading.

## Away-from-home VPN (Cloudflare WARP)

When `warp-cli` is installed and the device is enrolled in your Zero Trust organization, the
top bar shows `VPN` while the tunnel is up (`VPN..` while it is coming up) and a Wi-Fi state
(`Wi-Fi`, `Wi-Fi!` for no internet or a portal, `Wi-Fi...` joining, `No Wi-Fi`).

The tunnel connects by itself only away from home: the app learns the home network the first
time the node answers directly (a fingerprint of the Wi-Fi gateway's hardware address, kept in
`~/.local/state/endora-terminal/home-network`, never the address itself), and afterwards connects
only when the node has not answered for about 30 seconds on a network that is not home and
internet is available. On the home network it never connects and drops a tunnel it finds up,
because the tunnel routes private address ranges and would otherwise cut local access, including
SSH. Create `~/.config/endora-terminal/no-auto-vpn` to turn automatic connecting off.

## Desktop and back

Settings → Tools → `D` (unlocked only) shows the Raspberry Pi desktop on its own virtual terminal
and waits. Return with **Alt + Back** (or Alt + Backspace), or the "Back to Endora" icon on the
desktop and in its menu; over SSH, `sudo chvt 1` also works. The desktop is started for the visit
and stopped when you return; nothing is enabled at boot. The Q20 has no function keys, so the
return chord is read from the keyboard's raw events by `desktop-watch.py`, which records no text.

## No Wi-Fi and Wi-Fi login pages

- **No network:** the passcode box says `No network | Esc opens Wi-Fi settings`, and Esc opens
  Settings → Wi-Fi, where `C` lists the networks in range (nothing is needed to unlock first).
- **Login pages ("accept" portals):** when the network turns into a captive portal and the device
  is unlocked, it offers to open the text login (answer Y). While locked, the passcode box says
  `Wi-Fi login needed | Esc, then L`. In Settings → Wi-Fi, `L` opens the same text login at any time.
  It is a text browser (lynx) running in the same sandbox idea as the graphical portal browser:
  it cannot see the device's files, sessions or keys, and cookies vanish when it closes. Arrows
  move between links, Enter follows one, `Q` then `Y` quits. When you quit, the connection is
  checked again.
- **Pages that need scripts:** a text browser cannot run JavaScript. If a portal only works with
  it, use `G` in Settings → Wi-Fi (the graphical portal browser) or a phone hotspot.
- **General text browsing:** Settings → Tools → `W` (unlocked only) asks for an address and opens it in
  the same sandboxed text browser.

## Volume keys

The key left of the trackpad lowers the volume and the BlackBerry key raises it (5% a step, held
keys repeat, never above 100%), through `wpctl` on whatever output is current. These two keys send
Caps Lock and Super; the app loads a small console keymap (`loadkeys`) that makes them do nothing on
the console itself, so they no longer toggle caps lock, and reads them directly from the keyboard's
input device. The keymap resets at reboot and is applied again each time the app starts. They work
while the app is running (not inside the shell, browser or desktop sessions it opens).

## Restart from the keyboard

Hold the hang-up key (the rightmost key above the Q20 keyboard, which sends the pointer's right
button) for three seconds to restart the whole device; the status line counts down from the first
second and letting go early does nothing. It works from any page, even while locked, and is meant
for when the screen or an app misbehaves. It needs passwordless `sudo` (`systemctl reboot`).

## Passcode box

The passcode is never shown as you type (only a star per character). Press `Tab` in the box to show
what you typed, for example to check upper and lower case, and `Tab` again to hide it. The
handheld keyboard has no Tab key, so clicking its trackpad shows it and clicking again hides it. It hides again when you press
Enter or Esc. If caps lock is on, the box says so (the keyboard's caps-lock light is read from
`/sys/class/leds`) and `Ctrl+C` turns it off (the handheld's Alt key sends Ctrl, so Alt+C), using
`setleds -F -caps`. The handheld keyboard sends no Caps Lock key of its own.

## Text size

Press `+` to make the text larger and `-` to make it smaller (also in Settings → Display), on any
page except Endora AI and TV & Movies search (where those keys are typed) and Music (where they
change the volume). The size is a console font from Raspberry Pi OS (Small `Lat15-Terminus12x6`,
Normal `Uni2-VGA16`, then Terminus Bold 20, 24 and 28), so larger text shows fewer characters;
the choice is remembered. The console's boot font is built into the kernel and cannot be loaded
back from a file, which is why Normal is an ordinary 8x16 font. The Spotify sign-in QR codes need
half-block characters that most fonts lack, so sign-in switches to `Uni2-VGA14` while they are shown
and back to your size afterwards.

## Bluetooth audio

Bluetooth is invisible and unpairable unless a pairing window is open, so the device does not show
up on other people's lists (for example on a work network). Press Esc, then `B` (or Settings →
Bluetooth) for the Bluetooth page; Esc returns to where you came from:

- **P** opens a 2-minute pairing window on every Bluetooth adapter, with a live countdown. Each
  adapter shows its own name (`Pi Speaker USB` for a dongle, `Pi Speaker Built-in` for the board's own
  chip, `Pi Speaker` if unknown; a second of a kind is numbered), so pick one on the other device;
  **Esc** cancels it. It does not touch existing pairings, so many devices can be paired (phones,
  laptops, a Steam Deck). A device that pairs is trusted and then connected from this side too,
  which is what makes it appear as a speaker in its sound settings; the window closes a few
  seconds after a device pairs.
  If the Bluetooth chip does not answer (it sometimes stalls on this board, and the window then
  never becomes visible), the window notices and recovers by itself: it tries again, restarts
  Bluetooth, and finally reloads the Bluetooth driver, which resets the chip. Connected devices drop
  during a reset, and the page says what it is doing.
- **S** looks for a speaker or headphones to play audio *on* (12-second scan); pick one with
  Up/Down and Enter.
- **Up/Down + Enter** connects or disconnects a paired device; **X** forgets the selected one.
- **F** forgets all pairings (asks first). **R** restarts Bluetooth and the audio session (for when it
  gets stuck). **T** turns Bluetooth on or off.

A pairing exists on both sides: if you remove the device here, also remove it on the phone or laptop,
and the other way round, or they hold different keys and cannot reconnect (it then shows
"connected" with no speaker). Several connected devices can play at once; the audio system mixes
their streams into the one output, but this small board shares one radio between Wi-Fi and
Bluetooth, so expect two or three devices to work well, not many streaming together.

**Battery:** the Bluetooth radio is on only while the device is on your home network. On any other
Wi-Fi, or with no Wi-Fi, it turns itself off (after the situation has held for about 30 seconds, so a
brief Wi-Fi flicker never drops a headset). `T` turns it on by hand, and that stays until the
network situation really changes. The top bar shows `BT` while a Bluetooth device is connected.
While at home, every 30 seconds or so the app turns back on an adapter that dropped out (a power
change on the battery board resets the USB adapters) unless you turned Bluetooth off by hand, and
opens the audio link to a connected phone or Mac that has none. **A** plays a left, right, both
sound test through the current output.

## Audio dongle recovery

Switching the battery board between charger and battery briefly cuts power to the USB hub, and the
USB audio dongle can come back half-started (the device is there but has no sound card). The
app listens for the kernel's USB and sound-card announcements (`udevadm monitor`) and for audio
outputs appearing or disappearing (`pactl subscribe`). A few seconds after such an event settles it
checks a dongle that was working (and once a minute as a safety net). A dropout is first given
about fifteen seconds to fix itself, because a power dip often does and restarting the audio session
drops the Bluetooth stream; only if it persists does it act. If its sound card is gone but
the device is still on the bus, it switches that one device off and on (up to four times, like a
replug) and restarts the audio session; if the card is there but the audio server has no output, it
restarts the audio session twice and then resets the dongle twice. After restarting the audio session
it reopens the Bluetooth audio link to a connected phone or Mac (which may still need you to press
play). After acting it looks again a few seconds later. If the dongle refuses the reset (a wedged USB port answers "Protocol error"), or if
nothing brings it back, the status line asks you to unplug and replug the dongle, once. A
dongle that is simply unplugged, or that has never worked since the app started, is left alone.
The device reset needs passwordless `sudo`.

## Music (Spotify)

The Music page (Enter, then **S**) controls Spotify on this device's own Spotify Connect speaker
(named `Speaker`, played through the current audio output). Search and playback commands go to
Spotify's Web API, and sound comes from the device, so it works with the screen asleep. It needs
Spotify Premium and a free Spotify developer app (Web API only) whose redirect URI is exactly
`http://127.0.0.1:8888/callback`.

- **Sign in once (one QR code per sign-in):** on the Music page press `Y` (or Settings → Accounts →
  `S`, or run `endora-terminal music setup`) and give the app's Client ID. Each sign-in shows one QR
  code; scanning it opens a small page the device serves on your local network at a secret address
  (only while setup waits, up to 5 minutes). On it, tap "Open the Spotify sign-in", sign in and
  press Agree; the page after Agree will not load, which is expected: copy its address, come back to
  the first tab and paste it. Step 1 signs the app in for control; step 2 signs in this device's
  speaker, so it is on your account by itself after every reboot and the handheld can play without
  a phone choosing it (no Spotify app needed anywhere else). When Spotify is already signed in,
  setup asks first: `S` signs in only the speaker, `A` signs in again (needed once for Like). If
  your phone can't reach the device, the sign-in address is printed to open in any browser and the
  final address is typed on the handheld. No client secret is used (PKCE). The Client ID and the
  sign-in tokens are kept only in `~/.local/state/endora-terminal/spotify`, mode 0600, and the
  speaker's saved sign-in in `~/.local/state/librespot`, never in the repository.
- **The speaker:** `librespot` runs as the `spotify-speaker` user service with the PulseAudio
  backend, so its sound goes through PipeWire and mixes with Bluetooth streams (a Mac or phone
  playing to the device) into the current output. It replaces the old `raspotify` system service,
  which played straight to the USB audio device and so could not play at the same time; that service
  is only disabled (`sudo systemctl enable --now raspotify` brings it back). The installer does not
  set this service up yet.
- **Keys:** the page is a menu like the home screen. Playback: Space play or pause, `N` next,
  `B` previous, `+`/`-` volume. Browse (Up/Down + Enter, or the letter): `/` search, `D` discover,
  `L` playlists, `Y` liked songs, `A` saved albums, `T` top artists, `K` top tracks, `R` recently
  played. In a list, Enter plays and Esc goes back to the Music menu.
- **Discover radio (`D`):** an endless stream of songs you have not saved yet, from albums by the
  artists you play (two songs per album, about 20 per batch). It starts playing at once; when the
  last queued song starts, the next batch is added to Spotify's queue, so it keeps going (also with
  the screen off or on another page). `H` likes the song playing now (saves it to Liked songs),
  `X` skips it and plays less of that artist for the rest of the session, Enter on a listed song
  plays from there, and playing anything else ends the radio. Spotify no longer offers its
  recommendation or radio feeds to personal apps, and its DJ and Radio cannot be started from
  here, so the stream is built from your own listening. Saving songs needs the "library modify"
  permission and top artists the "top read" permission: if the page asks, sign in again once
  (Settings → Accounts → `S`).
- **Like and dislike (`H`, `X`):** on any song, from the Music page (listed under Playback). `H`
  saves the song to Liked songs; `X` skips it and leaves that artist out of the Discover radio and
  smart shuffle for the rest of the session.
- **Smart shuffle (`S`):** shuffles the playlist highlighted in your playlists (`L`), or Liked songs
  otherwise (also a row in the Music menu), and mixes in one new song from the Discover pool after
  every three of yours. Big playlists are sampled from a few random pages. It keeps going like the
  Discover radio, with the same `H` like and `X` skip. Spotify's own Smart Shuffle and Discover
  Weekly cannot be started from a personal app, so this is the handheld's version.
- **Play on (`O`):** lists the devices on your Spotify account that are ready to play (phones,
  computers, smart speakers, TVs, and this handheld as "Speaker"); Enter moves the music there and
  keeps playing. Later play, next and search go to that device too (remembered between runs);
  picking this handheld goes back to it, and if the chosen device is off, the handheld is used.
  AirPlay and HomeKit speakers (HomePods) are not Spotify devices and are not listed.
- **Other lists:** `A` saved albums, `T` top artists, `K` top tracks; Enter on an artist opens their
  albums (Esc goes back), Enter on an album plays it. Search also finds artists.
- **Without the speaker sign-in:** Spotify can only start playback on a Connect device it can see,
  so pick `Speaker` once in the Spotify app on a phone; steps 3–4 above make that unnecessary.

