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
        return Ok((std::fs::read(&cert_path)?, std::fs::read(&key_path)?));
    }
    let certified = rcgen::generate_simple_self_signed(sans.to_vec())?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();
    std::fs::create_dir_all(dir).ok();
    std::fs::write(&cert_path, &cert_pem)?;
    std::fs::write(&key_path, &key_pem)?;
    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}
