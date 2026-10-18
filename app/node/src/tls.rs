//! Optional self-signed HTTPS for the node.
//!
//! When `ENDORA_TLS=1`, the node serves HTTPS with a self-signed certificate so
//! the web console is a **secure context** — which browsers require for the
//! microphone (voice input). This needs no domain, CA, or reverse proxy; the
//! trade-off is a one-time "not trusted" warning per browser. The cert/key are
//! persisted next to the database so the same cert is reused across restarts
//! (the warning is accepted once, not every restart). Extra names/IPs for the
//! certificate come from `ENDORA_TLS_SAN` (comma-separated), e.g. the LAN IP.

use std::path::Path;

/// Loads a persisted self-signed cert/key from `dir`, generating and saving them
/// (covering `sans`) if absent. Returns `(cert_pem, key_pem)`.
///
/// # Errors
/// If the certificate cannot be generated, read, or written.
pub fn load_or_generate(
    dir: &Path,
    sans: &[String],
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let cert_path = dir.join("tls-cert.pem");
    let key_path = dir.join("tls-key.pem");
    if cert_path.exists() && key_path.exists() {
        // A key an older node wrote with the default umask is made owner-only on the way in.
        owner_only(&key_path)?;
        return Ok((std::fs::read(&cert_path)?, std::fs::read(&key_path)?));
    }
    let certified = rcgen::generate_simple_self_signed(sans.to_vec())?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();
    std::fs::create_dir_all(dir).ok();
    std::fs::write(&cert_path, &cert_pem)?;
    write_owner_only(&key_path, key_pem.as_bytes())?;
    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}

/// Writes `bytes` to a file created owner-only (`0600`), so the private key is never
/// readable by anyone else, not even for the moment before a `chmod`. Whatever was there
/// (a key without its certificate) is replaced.
fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Makes an existing file owner-only.
fn owner_only(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::load_or_generate;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("endora-tls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_private_key_is_written_owner_only() {
        // Written with the default umask it was 0644: anyone on the host could read the key
        // and stand in for the node.
        let dir = scratch("fresh");
        load_or_generate(&dir, &["localhost".to_owned()]).unwrap();
        assert_eq!(mode(&dir.join("tls-key.pem")), 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_left_readable_by_an_older_node_is_made_owner_only() {
        let dir = scratch("older");
        let (_, key) = load_or_generate(&dir, &["localhost".to_owned()]).unwrap();
        let key_path = dir.join("tls-key.pem");
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let (_, again) = load_or_generate(&dir, &["localhost".to_owned()]).unwrap();

        assert_eq!(again, key, "the same key is reused");
        assert_eq!(mode(&key_path), 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
