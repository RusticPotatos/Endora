use crate::{
    Result,
    client::Client,
    storage::{Config, Paths, Session},
    vault,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub desk: bool,
    #[serde(default)]
    pub remote: Option<Config>,
    #[serde(default)]
    pub away: bool,
    #[serde(default)]
    pub ssh: Option<String>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}
#[derive(Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub url: String,
}
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Secrets {
    pub token: String,
    pub access: Option<String>,
}
impl Settings {
    pub fn load() -> Result<Self> {
        let paths = Paths::config()?;
        if !paths.root.join("portable.toml").exists() {
            return Ok(Self::default());
        }
        toml::from_str(
            std::str::from_utf8(&paths.read("portable.toml")?)
                .map_err(|_| "Invalid portable settings")?,
        )
        .map_err(|_| "Invalid portable settings".into())
    }
    pub fn save(&self) -> Result<()> {
        if let Some(config) = &self.remote {
            config.validate()?;
            if !config.url.starts_with("https://") {
                return Err("Remote access requires HTTPS".into());
            }
        }
        for bookmark in &self.bookmarks {
            validate_web(&bookmark.url)?;
        }
        Paths::config()?.write(
            "portable.toml",
            toml::to_string(self)
                .map_err(|_| "Cannot encode portable settings")?
                .as_bytes(),
        )
    }
    pub fn config(&self) -> Result<Config> {
        if self.away {
            return self
                .remote
                .clone()
                .ok_or("Remote access needs private setup".into());
        }
        Paths::load_config()
    }
    pub fn file(&self, suffix: &str) -> String {
        format!("{}.{suffix}", if self.away { "remote" } else { "local" })
    }
}
pub fn validate_web(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|_| "Invalid browser destination")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use an HTTP(S) browser destination without credentials".into());
    }
    Ok(())
}
pub fn unlock(settings: &Settings, passphrase: &str) -> Result<Client> {
    let config = settings.config()?;
    let bytes = Paths::state()?.read(&settings.file("vault"))?;
    let plain = vault::open(passphrase, &config.url, &bytes)?;
    let secrets: Secrets =
        serde_json::from_slice(&plain).map_err(|_| "Invalid encrypted session")?;
    Client::new(&config, Some(secrets.token.clone()))?.with_access(secrets.access.clone())
}
pub fn locked(settings: &Settings) -> Result<Client> {
    let config = settings.config()?;
    let bytes = Paths::state()?.read(&settings.file("dashboard"))?;
    let session: Session =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid dashboard credential")?;
    if session.node != config.url {
        return Err("Dashboard credential belongs to another node".into());
    }
    Client::new(&config, Some(session.token))
}
pub async fn save(settings: &Settings, passphrase: &str, secrets: Secrets) -> Result<()> {
    let config = settings.config()?;
    let bytes = Zeroizing::new(serde_json::to_vec(&secrets).map_err(|_| "Cannot encode session")?);
    let encrypted = vault::seal(passphrase, &config.url, &bytes)?;
    let client =
        Client::new(&config, Some(secrets.token.clone()))?.with_access(secrets.access.clone())?;
    let result = client
        .post("/v1/session/dashboard", &serde_json::json!({}))
        .await?;
    let token = result["token"]
        .as_str()
        .ok_or("Node needs dashboard credential support")?;
    Paths::state()?.write(&settings.file("vault"), &encrypted)?;
    Paths::state()?.write(
        &settings.file("dashboard"),
        &serde_json::to_vec(&Session {
            node: config.url,
            token: token.to_owned(),
        })
        .map_err(|_| "Cannot encode dashboard credential")?,
    )?;
    Ok(())
}
