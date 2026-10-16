use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    #[serde(default)]
    pub allow_http: bool,
    pub ca_file: Option<PathBuf>,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid node URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Use an HTTP(S) node URL without credentials, query or fragment".into());
        }
        if url.scheme() == "http" && !self.allow_http {
            return Err("HTTP requires an explicit private-network choice in setup".into());
        }
        Ok(())
    }
}

pub struct Paths {
    pub root: PathBuf,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub node: String,
    pub token: String,
}

fn check(path: &Path, directory: bool) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Private file is unavailable")?;
    if meta.file_type().is_symlink()
        || meta.is_dir() != directory
        || (!directory && !meta.is_file())
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err("Unsafe private file ownership, permissions or symlink".into());
    }
    Ok(())
}

impl Paths {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn config() -> Result<Self> {
        Self::home(".config/endora-terminal")
    }
    pub fn state() -> Result<Self> {
        Self::home(".local/state/endora-terminal")
    }

    pub fn home(suffix: &str) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
        let root = PathBuf::from(home).join(suffix);
        // Reject symlinked application paths, including pre-existing parents.
        for path in root.ancestors().take(3) {
            if let Ok(meta) = fs::symlink_metadata(path) {
                if meta.file_type().is_symlink() {
                    return Err("Symlinked private directory refused".into());
                }
            }
        }
        if !root.exists() {
            fs::create_dir_all(&root).map_err(|_| "Cannot create private directory")?;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Cannot secure directory")?;
        }
        check(&root, true)?;
        Ok(Self::at(root))
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        if name.contains('/') || matches!(name, "" | "." | "..") {
            return Err("Invalid private filename".into());
        }
        check(&self.root, true)?;
        let path = self.root.join(name);
        check(&path, false)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(path)
            .map_err(|_| "Cannot read private file")?;
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read private file")?;
        if bytes.len() > 64 * 1024 {
            return Err("Private file exceeds budget".into());
        }
        Ok(bytes)
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if name.contains('/') || matches!(name, "" | "." | "..") {
            return Err("Invalid private filename".into());
        }
        check(&self.root, true)?;
        let dest = self.root.join(name);
        if fs::symlink_metadata(&dest).is_ok() {
            check(&dest, false)?;
        }
        let temp = self.root.join(format!(".{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temp)
                .map_err(|_| "Cannot create private file")?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| "Cannot save private file")?;
            fs::rename(&temp, dest).map_err(|_| "Cannot replace private file")?;
            Ok(())
        })();
        let _ = fs::remove_file(temp);
        result
    }

    pub fn load_config() -> Result<Config> {
        let bytes = Self::config()?.read("config.toml")?;
        let config: Config =
            toml::from_str(std::str::from_utf8(&bytes).map_err(|_| "Invalid configuration")?)
                .map_err(|_| "Invalid configuration; run setup")?;
        config.validate()?;
        Ok(config)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        if name.contains('/') || matches!(name, "" | "." | "..") {
            return Err("Invalid private filename".into());
        }
        check(&self.root, true)?;
        let path = self.root.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            check(&path, false)?;
            fs::remove_file(path).map_err(|_| "Cannot remove private file")?;
        }
        Ok(())
    }

    pub fn token(config: &Config) -> Result<Option<String>> {
        let paths = Self::state()?;
        if fs::symlink_metadata(paths.root.join("session")).is_err() {
            return Ok(None);
        }
        let session: Session = serde_json::from_slice(&paths.read("session")?)
            .map_err(|_| "Invalid session file; sign in again")?;
        if session.node.trim_end_matches('/') != config.url.trim_end_matches('/') {
            return Err("Session belongs to another node; sign in again".into());
        }
        Ok(Some(session.token))
    }

    pub fn logout() -> Result<()> {
        let paths = Self::state()?;
        for name in [
            "session",
            "local.vault",
            "remote.vault",
            "local.dashboard",
            "remote.dashboard",
        ] {
            let file = paths.root.join(name);
            if fs::symlink_metadata(&file).is_ok() {
                check(&file, false)?;
                fs::remove_file(file).map_err(|_| "Cannot remove local session")?;
            }
        }
        Ok(())
    }
}
