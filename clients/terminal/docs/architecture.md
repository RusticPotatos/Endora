# Terminal client boundary

The public repository builds a replaceable Rust HTTP client. Ratatui/Crossterm
handles the existing OS terminal; Tokio/Reqwest with Rustls handles network I/O.
Serde/TOML define wire and private configuration boundaries. UUID supplies request
identifiers, rpassword handles masked entry, rustix supports filesystem checks,
and unicode-width aligns text and the cursor. No dependency enters a server domain.

The server owns dashboard integration secrets and normalized observations.
The client has only its Endora session. No model output becomes shell commands.
Shell access is a deliberate keyboard action using the user's existing shell.

All runtime configuration and boot state live outside Git. Installers use a fixed
archive allowlist, versioned releases and atomic symlink replacement. Boot setup
preserves driver/network configuration and records the previous console state.
Automatic tty1 login trades physical login protection for an appliance experience;
SSH authentication is unchanged. There is no background network listener on the Pi.
