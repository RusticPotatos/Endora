use crate::{Result, protocol::Sse, storage::Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<std::sync::Arc<zeroize::Zeroizing<String>>>,
    access: Option<std::sync::Arc<zeroize::Zeroizing<String>>>,
}

#[derive(Clone, Deserialize, Serialize, Default)]
pub struct Message {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub actions: Value,
}

#[derive(Deserialize)]
pub struct Dashboard {
    pub services: Vec<Service>,
}

#[derive(Deserialize)]
pub struct Service {
    pub name: String,
    pub state: String,
    pub error: Option<String>,
    pub total: Option<u64>,
    #[serde(default)]
    pub items: Vec<LabItem>,
}

#[derive(Deserialize)]
pub struct LabItem {
    pub name: Option<String>,
    pub state: Option<String>,
    pub health: Option<String>,
    pub title: Option<String>,
    pub progress: Option<f64>,
    pub status: Option<String>,
}

/// The node's connection routes (`/v1/connections/…`) say what went wrong in their own words.
fn own_words(path: &str) -> bool {
    path.starts_with("/v1/connections/")
}

impl Client {
    pub fn new(config: &Config, token: Option<String>) -> Result<Self> {
        config.validate()?;
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120));
        if let Some(path) = &config.ca_file {
            let pem = std::fs::read(path).map_err(|_| "Cannot read CA certificate")?;
            builder = builder.add_root_certificate(
                reqwest::Certificate::from_pem(&pem).map_err(|_| "Invalid CA certificate")?,
            );
        }
        Ok(Self {
            http: builder
                .build()
                .map_err(|_| "Cannot initialize HTTP client")?,
            base: config.url.trim_end_matches('/').into(),
            token: token.map(|s| std::sync::Arc::new(zeroize::Zeroizing::new(s))),
            access: None,
        })
    }

    pub fn with_access(mut self, access: Option<String>) -> Result<Self> {
        if access.is_some() && !self.base.starts_with("https://") {
            return Err("Cloudflare authentication requires HTTPS".into());
        }
        self.access = access.map(|s| std::sync::Arc::new(zeroize::Zeroizing::new(s)));
        Ok(self)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(access) = &self.access {
            req = req.header("cf-access-token", access.as_str());
        }
        if let Some(token) = &self.token {
            return req.bearer_auth(token.as_str());
        }
        req
    }

    /// `own_words`: the request was to one of the node's connection routes, whose messages it
    /// writes itself, so a short one is shown as written.
    async fn response(req: reqwest::RequestBuilder, own_words: bool) -> Result<reqwest::Response> {
        let response = req
            .send()
            .await
            .map_err(|_| "Connection failed or timed out")?;
        match response.status().as_u16() {
            200..=299 => Ok(response),
            401 => Err("Session expired or rejected; run setup to sign in".into()),
            302 | 303 | 307
                if response
                    .headers()
                    .get("location")
                    .and_then(|h| h.to_str().ok())
                    .is_some_and(|s| {
                        s.contains("/cdn-cgi/access/") || s.contains("cloudflareaccess.com")
                    }) =>
            {
                Err("Cloudflare sign-in required".into())
            }
            403 => Err(
                "Credential does not permit this request; check Cloudflare sign-in if away".into(),
            ),
            404 => Err("Endpoint unavailable; dashboard requires a newer node".into()),
            code @ (400 | 409 | 500 | 502 | 503) => {
                let mut response = response;
                let mut bytes = Vec::new();
                while let Some(chunk) =
                    response.chunk().await.map_err(|_| "Response interrupted")?
                {
                    if bytes.len() + chunk.len() > 4096 {
                        return Err(format!("Node rejected request ({code})"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let body = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
                // The node reports "Home console: <category>"; older builds sent the bare category.
                let category = body["error"]
                    .as_str()
                    .map(|s| s.strip_prefix("Home console: ").unwrap_or(s));
                Err(match category {
                    Some("unconfigured") => "This feature needs private server setup".into(),
                    Some("request_user_unconfigured") => {
                        "Configure the media requesting user on the server".into()
                    }
                    Some("configuration") => {
                        "Server integration configuration needs attention".into()
                    }
                    Some("connection" | "upstream" | "protocol") => {
                        "Home service unavailable or returned an unexpected reply; try again later"
                            .into()
                    }
                    Some("authentication") => "Server integration sign-in needs attention".into(),
                    Some("protected_control_path") => {
                        "Protected service; use unlocked SSH for maintenance".into()
                    }
                    Some("request_conflict") => {
                        "Request identifier already used; check operation status".into()
                    }
                    Some("action_not_allowed" | "target_not_allowed") => {
                        "The node does not allow that action on that target".into()
                    }
                    Some("favorite_limit") => "Favorite limit reached".into(),
                    // A connection's setup ("No sign-in is waiting; start it again"): the node
                    // writes these itself rather than passing on what a provider said.
                    Some(s) if own_words && s.len() <= 100 && !s.chars().any(char::is_control) => {
                        s.to_owned()
                    }
                    Some("storage") => "Node storage unavailable".into(),
                    _ => format!("Node rejected request ({code})"),
                })
            }
            code => Err(format!("Node rejected request ({code})")),
        }
    }

    async fn json(req: reqwest::RequestBuilder, own_words: bool) -> Result<Value> {
        let mut response = Self::response(req, own_words).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "Response interrupted")? {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("Response exceeds display budget".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid node response".into())
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        Self::json(self.request(reqwest::Method::GET, path), own_words(path)).await
    }
    pub async fn post(&self, path: &str, value: &Value) -> Result<Value> {
        Self::json(
            self.request(reqwest::Method::POST, path).json(value),
            own_words(path),
        )
        .await
    }

    pub async fn sign_in(&self, password: String, code: String) -> Result<String> {
        let result = Self::json(
            self.request(reqwest::Method::POST, "/v1/session")
                .json(&json!({"password": password, "code": code})),
            false,
        )
        .await?;
        result["token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or("Sign-in did not return a session".into())
    }

    pub async fn chat(&self, message: String, events: mpsc::Sender<Value>) -> Result<()> {
        let request = self
            .request(reqwest::Method::POST, "/v1/chat/stream")
            .header("Accept", "text/event-stream")
            .json(&json!({"message": message, "request_id": uuid::Uuid::new_v4().to_string()}));
        let mut response = Self::response(request, false).await?;
        let mut sse = Sse::default();
        while let Some(bytes) = response
            .chunk()
            .await
            .map_err(|_| "Chat interrupted; outcome uncertain. Refresh history before resending")?
        {
            for event in sse.push(&bytes)? {
                let done = event["type"] == "done";
                let failed = event["type"] == "error";
                events.send(event).await.map_err(|_| "Chat closed")?;
                if done {
                    return Ok(());
                }
                if failed {
                    return Err(
                        "Node reported a chat error; refresh history before resending".into(),
                    );
                }
            }
        }
        Err(
            "Chat ended without completion; outcome uncertain. Refresh history before resending"
                .into(),
        )
    }
}
