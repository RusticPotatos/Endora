//! A thin blocking HTTP client for the node.
//!
//! The CLI holds no authority of its own; it just translates commands into
//! requests against the node's versioned API. Non-2xx responses are returned
//! (not turned into transport errors) so the node's JSON error body can be
//! shown to the user.

use serde_json::Value;

/// Errors the client can surface.
pub type ClientError = Box<dyn std::error::Error>;

/// A blocking client bound to a node base URL.
pub struct Client {
    agent: ureq::Agent,
    base: String,
}

impl Client {
    /// Creates a client for `base` (e.g. `http://127.0.0.1:8787`).
    #[must_use]
    pub fn new(base: String) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.into(),
            base,
        }
    }

    /// Issues a GET and returns the status code and parsed JSON body.
    ///
    /// # Errors
    /// A transport error, or a body that is not JSON.
    pub fn get(&self, path: &str) -> Result<(u16, Value), ClientError> {
        let mut res = self.agent.get(self.url(path)).call()?;
        let status = res.status().as_u16();
        let body = res.body_mut().read_json::<Value>()?;
        Ok((status, body))
    }

    /// Issues a POST with a JSON body and returns the status code and parsed
    /// JSON body.
    ///
    /// # Errors
    /// A transport error, or a body that is not JSON.
    pub fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), ClientError> {
        let mut res = self.agent.post(self.url(path)).send_json(body)?;
        let status = res.status().as_u16();
        let body = res.body_mut().read_json::<Value>()?;
        Ok((status, body))
    }

    /// Issues a DELETE and returns the status code and parsed JSON body.
    ///
    /// # Errors
    /// A transport error, or a body that is not JSON.
    pub fn delete(&self, path: &str) -> Result<(u16, Value), ClientError> {
        let mut res = self.agent.delete(self.url(path)).call()?;
        let status = res.status().as_u16();
        let body = res.body_mut().read_json::<Value>()?;
        Ok((status, body))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}
