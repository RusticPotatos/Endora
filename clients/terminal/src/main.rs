mod console;
use endora_terminal::{
    Result,
    client::Client,
    portable::{Secrets, Settings},
    storage::{Config, Paths},
};
use std::{
    io::{self, Write},
    process::Command,
};

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().map_err(|_| "Cannot display prompt")?;
    let mut value = String::new();
    if io::stdin()
        .read_line(&mut value)
        .map_err(|_| "Cannot read input")?
        == 0
    {
        return Err("Setup cancelled".into());
    }
    Ok(value.trim().to_owned())
}

async fn setup() -> Result<()> {
    let mut settings = Settings::load()?;
    let existing = settings.config().ok();
    let entered = prompt("Endora node URL (blank keeps configured node): ")?;
    let url = if entered.is_empty() {
        existing
            .as_ref()
            .map(|c| c.url.clone())
            .ok_or("Node URL is required")?
    } else {
        entered
    };
    let allow_http = if url.starts_with("http://") {
        println!(
            "HTTP sends credentials and conversation without encryption. Use only a private trusted network."
        );
        prompt("Use private-network HTTP? Type yes: ")? == "yes"
    } else {
        false
    };
    let ca = prompt("Local CA PEM path (blank keeps configured CA; '-' uses public trust): ")?;
    let ca_file = if ca == "-" {
        None
    } else if ca.is_empty() {
        existing
            .as_ref()
            .filter(|c| c.url == url)
            .and_then(|c| c.ca_file.clone())
    } else {
        Some(ca.into())
    };
    let config = Config {
        url,
        allow_http,
        ca_file,
    };
    let access = if settings.away {
        Some(cloud_login(&config.url)?)
    } else {
        None
    };
    let client = Client::new(&config, None)?.with_access(access.clone())?;
    client.get("/health").await?;
    let enrollment = client.get("/v1/session").await?;
    if enrollment["set_up"] != true {
        return Err(
            "Complete node enrollment through Endora's web console, then run setup again".into(),
        );
    }
    let password = rpassword::prompt_password("Endora password: ")
        .map_err(|_| "Cannot read masked password")?;
    let code = rpassword::prompt_password("Authenticator code: ")
        .map_err(|_| "Cannot read masked code")?;
    let token = client.sign_in(password, code).await?;
    // Configuration and credentials remain outside any repository.
    if settings.away {
        settings.remote = Some(config.clone());
        settings.save()?;
    } else {
        Paths::config()?.write(
            "config.toml",
            toml::to_string(&config)
                .map_err(|_| "Cannot encode configuration")?
                .as_bytes(),
        )?;
    }
    let passphrase = new_passphrase()?;
    endora_terminal::portable::save(&settings, &passphrase, Secrets { token, access }).await?;
    Paths::state()?.remove("session")?;
    println!(
        "Setup complete. Privileged sessions are encrypted; unlock with your device passphrase."
    );
    Ok(())
}

fn active(service: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .status()
        .is_ok_and(|s| s.success())
}

async fn doctor() -> Result<()> {
    println!("endora-terminal {}", env!("CARGO_PKG_VERSION"));
    println!("Architecture: {}", std::env::consts::ARCH);
    println!(
        "Terminal available: {}",
        crossterm::terminal::size().is_ok()
    );
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        println!("Terminal: {cols} columns × {rows} rows");
    }
    println!(
        "Framebuffer present: {}",
        std::path::Path::new("/sys/class/graphics/fb0").exists()
    );
    let keyboard = std::fs::read_dir("/dev/input/by-id").is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("HACKBerry"))
    });
    println!("HACKBerry keyboard present: {keyboard}");
    println!(
        "UTF-8 locale: {}",
        std::env::var("LANG")
            .unwrap_or_default()
            .to_lowercase()
            .contains("utf")
    );
    println!("NetworkManager active: {}", active("NetworkManager"));
    println!("SSH active: {}", active("ssh"));
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(stat) = rustix::fs::statvfs(home.as_str()) {
            println!(
                "Available disk MiB: {}",
                stat.f_bavail * stat.f_frsize / 1024 / 1024
            );
        }
    }
    match load_client() {
        Ok(client) => {
            println!("Node health: {}", client.get("/health").await.is_ok());
            println!(
                "Locked overview available: {}",
                client.get("/v1/homelab/overview").await.is_ok()
            );
        }
        Err(_) => println!("Connection not configured safely; run setup"),
    }
    Ok(())
}

fn load_client() -> Result<Client> {
    endora_terminal::portable::locked(&Settings::load()?)
}

fn unlock_client() -> Result<Client> {
    let passphrase = zeroize::Zeroizing::new(
        rpassword::prompt_password("Device passcode: ")
            .map_err(|_| "Cannot read device passphrase")?,
    );
    endora_terminal::portable::unlock(&Settings::load()?, &passphrase)
}
fn new_passphrase() -> Result<zeroize::Zeroizing<String>> {
    let first = zeroize::Zeroizing::new(
        rpassword::prompt_password("New device passcode (4+ characters; typing is hidden): ")
            .map_err(|_| "Cannot read device passphrase")?,
    );
    let second = zeroize::Zeroizing::new(
        rpassword::prompt_password("Repeat device passcode (typing is hidden): ")
            .map_err(|_| "Cannot read device passphrase")?,
    );
    if *first != *second {
        return Err("Passcodes did not match; try E again".into());
    }
    endora_terminal::vault::validate_passphrase(&first)?;
    Ok(first)
}
async fn lock_setup() -> Result<()> {
    let settings = Settings::load()?;
    let config = settings.config()?;
    let token = Paths::token(&config)?.ok_or("No legacy session; run setup instead")?;
    let passphrase = new_passphrase()?;
    endora_terminal::portable::save(
        &settings,
        &passphrase,
        Secrets {
            token,
            access: None,
        },
    )
    .await?;
    Paths::state()?.remove("session")?;
    println!("Session migrated to encrypted storage. Use U to unlock administrative actions.");
    Ok(())
}
fn bundle_script(name: &str) -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().map_err(|_| "Cannot locate release")?;
    let path = exe.parent().ok_or("Cannot locate release")?.join(name);
    if !path.is_file() {
        return Err("Install the release bundle to use device features".into());
    }
    Ok(path)
}
fn browser(mode: &str, url: &str) -> Result<()> {
    endora_terminal::portable::validate_web(url)?;
    let mut command = Command::new("sh");
    command.arg(bundle_script("browser.sh")?).args([mode, url]);
    if mode == "web" {
        return privileged_child(command);
    }
    let status = command.status().map_err(|_| "Cannot open browser")?;
    if !status.success() {
        return Err("Browser unavailable; use Wi-Fi settings or a phone hotspot".into());
    }
    Ok(())
}

/// One-time Spotify sign-in from this terminal: no secret is involved. You open a link on any
/// device, approve, and paste back the address the browser ends on.
/// True when a line has been typed (Enter pressed) on the terminal; the line is consumed.
fn enter_pressed() -> bool {
    use rustix::event::{PollFd, PollFlags, poll};
    let stdin = std::io::stdin();
    let mut fds = [PollFd::new(&stdin, PollFlags::IN)];
    if poll(&mut fds, 0).is_ok_and(|n| n > 0) {
        let mut line = String::new();
        let _ = stdin.read_line(&mut line);
        return true;
    }
    false
}

/// `node`: the unlocked client; the sign-in is made on Endora through it.
async fn music_setup(node: &Client) -> Result<()> {
    use endora_terminal::spotify as sp;
    // The sign-in QR codes need half-block characters that most console fonts lack, so a font with
    // them is used while they are on screen and the chosen size is restored afterwards.
    struct RestoreTextSize;
    impl Drop for RestoreTextSize {
        fn drop(&mut self) {
            let _ = endora_terminal::font::apply(endora_terminal::font::load());
        }
    }
    // The QR font is used whatever the text size, since not every font has those characters.
    let _restore = {
        let _ = endora_terminal::font::apply_font(endora_terminal::font::QR_FONT);
        RestoreTextSize
    };
    // The sign-in is Endora's, for every device (plan phase 4); this device's speaker is its own.
    let speaker_ready = speaker_signed_in();
    let status = |value: serde_json::Value| -> Result<endora_protocol::ConnectionStatus> {
        serde_json::from_value(value).map_err(|_| "Unexpected reply from Endora".into())
    };
    // A sign-in made here before Endora held one is handed to Endora by loading a session.
    if sp::holds_own() {
        let _ = sp::Spotify::load(Some(node)).await;
    }
    let now = status(node.get("/v1/connections/spotify").await?)?;
    if now.state == "connected" {
        println!(
            "\x1b[2J\x1b[H{}. Every device plays with it: this one and the web console.\n\n  S  Sign in this device's speaker only (one QR code){}\n  A  Connect again (replaces the sign-in on every device)\n\n  Enter cancels\n",
            now.detail,
            if speaker_ready {
                "; it is already signed in"
            } else {
                ""
            }
        );
        match prompt("Choice: ")?.trim().to_ascii_lowercase().as_str() {
            "s" => {
                speaker_setup(1, 1).await?;
                println!("Done. This device can now play Spotify by itself.");
                return Ok(());
            }
            "a" => {}
            _ => return Err("Spotify sign-in cancelled".into()),
        }
    }
    let total = if speaker_ready { 1 } else { 2 };
    // Endora keeps the client id it was given; one this device saved is offered when it has none.
    let saved = sp::saved_client_id().filter(|_| !now.client_id_set);
    let typed = prompt(if now.client_id_set || saved.is_some() {
        "Spotify Client ID (blank keeps the saved one): "
    } else {
        "Spotify Client ID: "
    })?;
    let client_id = match (typed.trim(), saved) {
        ("", Some(id)) => id,
        ("", None) if now.client_id_set => String::new(),
        ("", None) => return Err("A Client ID is needed".into()),
        (id, _) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => id.to_owned(),
        _ => return Err("A Client ID is 32 letters and digits".into()),
    };
    // Spotify sends the browser to a loopback address that does not load; it is pasted back.
    let begun = status(
        node.post(
            "/v1/connections/spotify/begin",
            &serde_json::json!({"client_id": client_id, "via": "paste"}),
        )
        .await?,
    )?;
    let url = begun
        .sign_in_url
        .ok_or("Endora did not start the sign-in")?;
    let pasted = qr_and_paste(
        &sp::step_heading(
            1,
            total,
            "scan with your phone's camera and follow the page.",
        ),
        &url,
    )
    .await?;
    let done = status(
        node.post(
            "/v1/connections/spotify/finish",
            &serde_json::json!({"returned": pasted.trim()}),
        )
        .await?,
    )?;
    if !speaker_ready {
        println!(
            "{}. Next: sign in this device's speaker so it can play by itself.",
            done.detail
        );
        speaker_setup(2, 2)
            .await
            .map_err(|e| format!("Spotify is connected, but the speaker sign-in failed: {e}"))?;
    }
    println!(
        "Done. Every device plays with this sign-in: the Music page here (Esc, then M) and the web console."
    );
    Ok(())
}

/// One sign-in, with a single QR code: it opens a page this device serves for a few minutes on the
/// local network at a secret path, with a button for the Spotify sign-in (the link itself is too
/// long for a QR code on this screen) and a box to paste the address of the page that would not
/// load after Agree. Without a local network address the link is printed and pasted by hand.
async fn qr_and_paste(heading: &str, url: &str) -> Result<String> {
    use endora_terminal::spotify as sp;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let listener = sp::local_ip().and_then(|ip| {
        std::net::TcpListener::bind(("0.0.0.0", 8899))
            .ok()
            .map(|l| (ip, l))
    });
    let Some((ip, listener)) = listener else {
        println!(
            "\x1b[2J\x1b[H{heading}\n\nOpen this address in a browser and sign in:\n\n{url}\n"
        );
        return prompt("Then paste the full address of the page that would not load: ");
    };
    let link = format!("http://{ip}:8899/{token}");
    let qr = sp::qr_lines(&link).unwrap_or_else(|_| vec![link.clone()]);
    print!(
        "{}",
        sp::qr_page(
            heading,
            &qr,
            &format!(
                "On that page: tap Open the Spotify sign-in, sign in, press Agree,\nthen paste the address of the page that would not load.\n(or open {link})  Waiting up to 5 minutes; Enter cancels."
            ),
        )
    );
    let url = url.to_owned();
    let (waited, stopped) = tokio::task::spawn_blocking(move || {
        let mut stopped = false;
        let waited = sp::wait_for_sign_in(
            &listener,
            &token,
            &url,
            std::time::Duration::from_secs(300),
            || {
                stopped = enter_pressed();
                stopped
            },
        );
        (waited, stopped)
    })
    .await
    .map_err(|_| "Setup was interrupted")?;
    if stopped {
        return Err("Spotify sign-in cancelled".into());
    }
    match waited {
        Ok(text) => Ok(text),
        Err(_) => prompt("Timed out. Paste the full address here instead: "),
    }
}

/// Whether this device's speaker has a saved Spotify sign-in.
fn speaker_signed_in() -> bool {
    std::env::var_os("HOME").is_some_and(|home| {
        let home = std::path::PathBuf::from(home);
        [".local/state/librespot", ".cache/librespot"]
            .iter()
            .any(|dir| home.join(dir).join("credentials.json").exists())
    })
}

async fn speaker_setup(first: u8, total: u8) -> Result<()> {
    use endora_terminal::spotify as sp;
    use std::io::{BufRead, Read, Write};
    const PORT: u16 = 5599;
    struct StartSpeaker;
    impl Drop for StartSpeaker {
        fn drop(&mut self) {
            let _ = Command::new("systemctl")
                .args(["--user", "start", "spotify-speaker"])
                .status();
        }
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME is unavailable")?);
    let cache = home.join(".cache/librespot");
    let system_cache = home.join(".local/state/librespot");
    for dir in [&cache, &system_cache] {
        std::fs::create_dir_all(dir).map_err(|_| "Cannot create the speaker's sign-in folder")?;
    }
    // librespot keeps its saved sign-in in the system cache folder (the cache folder otherwise).
    let saved = [
        system_cache.join("credentials.json"),
        cache.join("credentials.json"),
    ];
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "spotify-speaker"])
        .status();
    let _start = StartSpeaker;
    let mut child = Command::new("librespot")
        .args([
            "--enable-oauth",
            "--oauth-port",
            &PORT.to_string(),
            "--disable-discovery",
        ])
        .arg("--cache")
        .arg(&cache)
        .arg("--system-cache")
        .arg(&system_cache)
        .args([
            "--name",
            "Speaker",
            "--backend",
            "pipe",
            "--device",
            "/dev/null",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| "The Spotify player (librespot) is not installed")?;
    let (lines, read) = std::sync::mpsc::channel::<String>();
    let streams: Vec<Box<dyn Read + Send>> = vec![
        Box::new(child.stdout.take().ok_or("Cannot read the player")?),
        Box::new(child.stderr.take().ok_or("Cannot read the player")?),
    ];
    for stream in streams {
        let lines = lines.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stream)
                .lines()
                .map_while(|l| l.ok())
            {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
    }
    let started = std::time::Instant::now();
    let link = loop {
        match read.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(line) => {
                if let Some(link) = sp::speaker_link(&line) {
                    break Ok(link);
                }
            }
            Err(_) if started.elapsed() > std::time::Duration::from_secs(20) => {
                break Err("The player did not offer a sign-in link");
            }
            Err(_) => {}
        }
    };
    let finish = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    let link = match link {
        Ok(link) => link,
        Err(e) => {
            finish(&mut child);
            return Err(e.into());
        }
    };
    let result: Result<()> = async {
        let pasted = qr_and_paste(
            &sp::step_heading(
                first,
                total,
                "scan to let this device play Spotify by itself, and follow the page.",
            ),
            &link,
        )
        .await?;
        let path = sp::speaker_login_path(&pasted, PORT)?;
        // Hand the answer to the player's own sign-in listener on this device.
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", PORT))
            .map_err(|_| "The player stopped waiting for the sign-in")?;
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\n\r\n"
        )
        .map_err(|_| "Cannot reach the player's sign-in")?;
        let mut reply = Vec::new();
        let _ = stream.take(16 * 1024).read_to_end(&mut reply);
        let saved_by = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let signed_in = || saved.iter().any(|f| f.exists());
        while !signed_in() && std::time::Instant::now() < saved_by {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        if signed_in() {
            Ok(())
        } else {
            Err("The speaker did not save its sign-in".into())
        }
    }
    .await;
    finish(&mut child);
    result
}

/// A sandboxed text-mode browser in this terminal, for Wi-Fi login pages and quick reading.
fn text_browser(mode: &str, url: &str) -> Result<()> {
    endora_terminal::portable::validate_web(url)?;
    let status = Command::new("sh")
        .arg(bundle_script("text-browser.sh")?)
        .args([mode, url])
        .status()
        .map_err(|_| "Text browser unavailable")?;
    if status.success() {
        Ok(())
    } else {
        Err("Text browser needs the lynx and bubblewrap packages".into())
    }
}

/// Show the Raspberry Pi desktop; the script waits until Alt+Back (or the icon) returns here.
fn desktop() -> Result<()> {
    let status = Command::new("sh")
        .arg(bundle_script("desktop.sh")?)
        .status()
        .map_err(|_| "Desktop unavailable")?;
    if status.success() {
        Ok(())
    } else {
        Err("Desktop did not start; use SSH to check lightdm".into())
    }
}

fn privileged_child(mut command: Command) -> Result<()> {
    use std::{
        io::Read,
        os::unix::{fs::OpenOptionsExt, process::CommandExt},
        time::{Duration, Instant},
    };
    command.process_group(0);
    let mut activity = Vec::new();
    if endora_terminal::wheel::is_console() {
        if let Ok(entries) = std::fs::read_dir("/dev/input/by-id") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.contains("HACKBerry")
                    && (name.ends_with("event-kbd") || name.ends_with("event-mouse"))
                {
                    if let Ok(file) = std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
                        .open(entry.path())
                    {
                        activity.push(file);
                    }
                }
            }
        }
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot open privileged session")?;
    let mut last = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|_| "Cannot inspect session")? {
            return if status.success() {
                Ok(())
            } else {
                Err("Session ended or command failed".into())
            };
        }
        for file in &mut activity {
            let mut bytes = [0; 24 * 64];
            if file.read(&mut bytes).is_ok_and(|n| n > 0) {
                last = Instant::now();
            }
        }
        if last.elapsed() >= Duration::from_secs(300) {
            if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Term);
                std::thread::sleep(Duration::from_millis(250));
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Kill);
            }
            let _ = child.wait();
            return Err("Session closed after five minutes without device input".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn cloud_login(url: &str) -> Result<String> {
    endora_terminal::portable::validate_web(url)?;
    let output = Command::new("sh")
        .arg(bundle_script("browser.sh")?)
        .args(["auth", url])
        .output()
        .map_err(|_| "Cannot start Cloudflare login")?;
    let bytes = zeroize::Zeroizing::new(output.stdout);
    let token = std::str::from_utf8(&bytes)
        .map_err(|_| "Invalid Cloudflare sign-in response")?
        .trim();
    if !output.status.success()
        || token.split('.').count() != 3
        || token.contains(char::is_whitespace)
    {
        return Err("Cloudflare sign-in failed; check private remote setup".into());
    }
    Ok(token.to_owned())
}
fn wifi_picker() -> Result<()> {
    println!("Scanning Wi-Fi…");
    let rows = endora_terminal::network::scan()?;
    for (index, row) in rows.iter().enumerate() {
        println!(
            "{}  {}  {}% {}{}",
            index + 1,
            endora_terminal::protocol::clean(&row.ssid),
            row.signal,
            if row.secured { "secured" } else { "open" },
            if row.connected { " (connected)" } else { "" }
        );
    }
    let chosen = prompt("Network number (blank returns): ")?;
    if chosen.is_empty() {
        return Ok(());
    }
    let row = chosen
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|n| rows.get(n))
        .ok_or("Invalid network selection")?;
    let status = Command::new("nmcli")
        .args(["--ask", "device", "wifi", "connect", &row.ssid])
        .status()
        .map_err(|_| "Cannot connect Wi-Fi")?;
    if !status.success() {
        return Err("Wi-Fi connection failed; try nmtui recovery".into());
    }
    Ok(())
}
fn remote_setup() -> Result<()> {
    let mut settings = Settings::load()?;
    let url = prompt("Access-protected Endora HTTPS URL: ")?;
    let config = Config {
        url,
        allow_http: false,
        ca_file: None,
    };
    config.validate()?;
    if !config.url.starts_with("https://") {
        return Err("Remote access requires HTTPS".into());
    }
    let ssh = prompt("Existing SSH config alias for Cloudflare SSH (blank skips): ")?;
    if !ssh.is_empty()
        && (!ssh
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            || ssh.starts_with('-'))
    {
        return Err("Use an existing SSH config alias".into());
    }
    settings.remote = Some(config);
    settings.ssh = (!ssh.is_empty()).then_some(ssh);
    settings.away = true;
    settings.save()?;
    println!("Remote profile saved. Run setup to complete Cloudflare and Endora sign-in.");
    Ok(())
}

fn bookmark_setup() -> Result<()> {
    let mut settings = Settings::load()?;
    println!("Browser shortcuts are private and require device unlock.");
    for (i, bookmark) in settings.bookmarks.iter().enumerate() {
        println!(
            "{} {}",
            i + 1,
            endora_terminal::protocol::clean(&bookmark.name)
        );
    }
    let name = prompt("Shortcut name (blank returns): ")?;
    if name.is_empty() {
        return Ok(());
    }
    if name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err("Use a short printable shortcut name".into());
    }
    let url = prompt("HTTP(S) URL ('-' removes this named shortcut): ")?;
    if url != "-" {
        endora_terminal::portable::validate_web(&url)?;
    }
    settings.bookmarks.retain(|bookmark| bookmark.name != name);
    if url != "-" {
        if settings.bookmarks.len() >= 16 {
            return Err("Remove a shortcut first; maximum is 16".into());
        }
        settings
            .bookmarks
            .push(endora_terminal::portable::Bookmark { name, url });
    }
    settings.save()
}

fn boot(action: &str) -> Result<()> {
    if !matches!(action, "enable" | "disable") {
        return Err("Use boot enable or boot disable".into());
    }
    let home = std::env::var_os("HOME").ok_or("HOME unavailable")?;
    let script =
        std::path::PathBuf::from(home).join(".local/share/endora-terminal/current/boot.sh");
    if !script.is_file() {
        return Err("Install the release bundle before configuring boot".into());
    }
    let status = Command::new("sh")
        .arg(script)
        .arg(action)
        .status()
        .map_err(|_| "Cannot run boot installer")?;
    if !status.success() {
        return Err("Boot operation failed; inspect local recovery instructions".into());
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => console::run().await,
        ["setup"] => setup().await,
        ["lock-setup"] => lock_setup().await,
        ["remote-setup"] => remote_setup(),
        ["bookmarks"] => unlock_client().and_then(|_| bookmark_setup()),
        ["wifi"] => wifi_picker(),
        ["profile", name @ ("home" | "away")] => (|| {
            let mut settings = Settings::load()?;
            settings.away = *name == "away";
            settings.config()?;
            settings.save()
        })(),
        ["browser", mode @ ("portal" | "web"), url] => {
            if *mode == "web" {
                unlock_client().and_then(|_| browser(mode, url))
            } else {
                browser(mode, url)
            }
        }
        ["doctor"] => doctor().await,
        ["music", "setup"] => match unlock_client() {
            Ok(node) => music_setup(&node).await,
            Err(e) => Err(e),
        },
        ["power", "report"] => endora_terminal::telemetry::History::standard().map(|h| {
            println!("{}", endora_terminal::telemetry::report(&h.load()));
        }),
        ["logout"] => Paths::logout()
            .map(|()| println!("Local session removed; server session was not revoked")),
        ["health"] => match load_client() {
            Ok(client) => client
                .get("/health")
                .await
                .map(|_| println!("Node healthy")),
            Err(e) => Err(e),
        },
        ["boot", action] => boot(action),
        ["--help"] | ["help"] => {
            println!(
                "endora-terminal [setup|lock-setup|remote-setup|bookmarks|profile home|profile away|wifi|doctor|logout|health|boot enable|boot disable]"
            );
            Ok(())
        }
        _ => Err("Unknown command; use --help".into()),
    };
    if let Err(error) = result {
        eprintln!("{}", endora_terminal::protocol::clean(&error));
        std::process::exit(1);
    }
}
