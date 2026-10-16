//! Spotify, held by Endora: one sign-in for every device (plan phase 4, ADR 0099).
//!
//! The handheld signed in to Spotify on its own, keeping the tokens in a file on the device. A
//! second device signing in separately would fight it: Spotify may rotate the refresh token each
//! time it is used, so two holders knock each other out. So Endora holds the sign-in (the client
//! id and the refresh token, with the other connections) and **lends** an access token, good for
//! an hour, to whichever device asks: the web console, the handheld, or a skill in chat. The
//! refresh token never leaves the node.
//!
//! The sign-in itself (authorization code with PKCE, no client secret) is kept as a pending
//! state here, so it can be started on one device and finished on another, and both show where
//! it stands. Spotify sends the person back either to Endora's own callback, which completes it
//! by itself, or to a loopback address that does not load, whose address they paste into either
//! device.

use super::CapabilitySettings;
use crate::application::CapabilitySettingsRepository;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Where the sign-in and its tokens are kept, beside the other connections' settings.
pub const CONNECTION_ID: &str = "spotify";

/// What Endora may do with the account: read and control playback, read playlists and the
/// library, save a song. The handheld's Music page uses exactly these.
pub const SCOPES: &str = "user-read-playback-state user-modify-playback-state playlist-read-private user-library-read user-read-recently-played user-top-read user-library-modify";

/// The loopback address Spotify sends the person to when Endora's own callback is not used. It
/// does not load; its address is pasted back into either device. Spotify accepts loopback over
/// plain HTTP.
pub const PASTE_REDIRECT: &str = "http://127.0.0.1:8888/callback";

/// Endora's own callback, outside `/v1`: a browser redirect carries no bearer token, so the
/// single-use `state` of the pending sign-in is what authorizes it.
pub const CALLBACK_PATH: &str = "/connect/spotify/callback";

/// How long a started sign-in waits to be finished.
pub const PENDING_FOR_MS: i64 = 15 * 60 * 1000;

/// A lent access token is refreshed when less than this is left of it.
const REFRESH_WITHIN_MS: i64 = 60 * 1000;

/// The PKCE challenge for a verifier: SHA-256, URL-safe base64 without padding.
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The address the person opens to approve Endora on Spotify.
#[must_use]
pub fn sign_in_url(client_id: &str, challenge: &str, state: &str, redirect: &str) -> String {
    format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge_method=S256&code_challenge={}&state={}",
        encode(client_id),
        encode(redirect),
        encode(SCOPES),
        encode(challenge),
        encode(state),
    )
}

/// The authorization code from what the person pasted: the whole address Spotify sent them to,
/// or just its query, checked against the pending sign-in's `state`.
///
/// # Errors
/// A sentence for the person: Spotify said no, the address belongs to another sign-in, or there
/// is no code in it.
pub fn code_from(pasted: &str, state: &str) -> Result<String, &'static str> {
    let query = pasted
        .trim()
        .split_once('?')
        .map_or(pasted.trim(), |(_, q)| q);
    let field = |name: &str| {
        query.split('&').find_map(|pair| {
            pair.split_once('=')
                .filter(|(k, _)| *k == name)
                .map(|(_, v)| v.to_owned())
        })
    };
    if field("error").is_some() {
        return Err("Spotify did not approve the sign-in");
    }
    if field("state").as_deref() != Some(state) {
        return Err("That address belongs to another sign-in; start again");
    }
    field("code")
        .filter(|c| !c.is_empty() && c.len() < 1024)
        .ok_or("There is no sign-in code in that address")
}

/// Spotify's accounts service: the two calls a connection needs. Behind a trait so the rules
/// here are tested without Spotify.
pub trait SpotifyAccounts {
    /// `POST https://accounts.spotify.com/api/token` with a form.
    ///
    /// # Errors
    /// A sentence for the person if Spotify cannot be reached or refuses.
    fn token(&self, form: &[(&str, &str)]) -> Result<Value, &'static str>;
    /// `GET https://api.spotify.com/v1/me` with an access token: who is signed in.
    ///
    /// # Errors
    /// A sentence for the person if Spotify cannot be reached or refuses.
    fn me(&self, access_token: &str) -> Result<Value, &'static str>;
}

/// The real accounts service, over HTTPS.
pub struct Accounts;

impl SpotifyAccounts for Accounts {
    fn token(&self, form: &[(&str, &str)]) -> Result<Value, &'static str> {
        let body: String = form
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let mut response = super::agent()
            .post("https://accounts.spotify.com/api/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .config()
            .http_status_as_error(false)
            .build()
            .send(body.as_bytes())
            .map_err(|_| "Spotify could not be reached")?;
        if !response.status().is_success() {
            return Err("Spotify refused; sign in again");
        }
        response
            .body_mut()
            .read_json()
            .map_err(|_| "Spotify answered something unexpected")
    }

    fn me(&self, access_token: &str) -> Result<Value, &'static str> {
        let mut response = super::agent()
            .get("https://api.spotify.com/v1/me")
            .header("Authorization", &format!("Bearer {access_token}"))
            .call()
            .map_err(|_| "Spotify could not be reached")?;
        response
            .body_mut()
            .read_json()
            .map_err(|_| "Spotify answered something unexpected")
    }
}

/// The Spotify connection, kept in the connection settings.
pub struct Connection<'a> {
    settings: &'a dyn CapabilitySettingsRepository,
    accounts: &'a dyn SpotifyAccounts,
}

impl<'a> Connection<'a> {
    /// The connection over these settings and accounts service.
    #[must_use]
    pub fn new(
        settings: &'a dyn CapabilitySettingsRepository,
        accounts: &'a dyn SpotifyAccounts,
    ) -> Self {
        Self { settings, accounts }
    }

    fn all(&self) -> CapabilitySettings {
        self.settings
            .all_settings()
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _, _)| id == CONNECTION_ID)
            .map(|(_, k, v)| (k, v))
            .collect()
    }

    fn get(&self, key: &str) -> String {
        self.all().get(key).cloned().unwrap_or_default()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), &'static str> {
        self.settings
            .set_setting(CONNECTION_ID, key, value)
            .map_err(|_| "Endora could not save the Spotify connection")
    }

    fn clear_pending(&self) -> Result<(), &'static str> {
        for key in [
            "pending_state",
            "pending_verifier",
            "pending_redirect",
            "pending_started_ms",
            "pending_url",
        ] {
            self.set(key, "")?;
        }
        Ok(())
    }

    /// Where the connection stands, for both devices: `not_set_up`, `waiting` (a sign-in was
    /// started and not finished, with the address to open), or `connected` (with the account).
    #[must_use]
    pub fn status(&self, now_ms: i64) -> Value {
        let s = self.all();
        let get = |k: &str| s.get(k).cloned().unwrap_or_default();
        let started = get("pending_started_ms").parse::<i64>().unwrap_or(0);
        let waiting = !get("pending_state").is_empty() && now_ms - started < PENDING_FOR_MS;
        let connected = !get("refresh_token").is_empty();
        let state = if waiting {
            "waiting"
        } else if connected {
            "connected"
        } else {
            "not_set_up"
        };
        let detail = match state {
            "waiting" => "Waiting for you to approve Endora on Spotify".to_owned(),
            "connected" => {
                let account = get("account");
                if account.is_empty() {
                    "Connected".to_owned()
                } else {
                    format!("Connected as {account}")
                }
            }
            _ => "Not connected".to_owned(),
        };
        json!({
            "state": state,
            "detail": detail,
            "client_id_set": !get("client_id").is_empty(),
            "sign_in_url": waiting.then(|| get("pending_url")),
            "redirect": waiting.then(|| get("pending_redirect")),
            "started_ms": waiting.then_some(started),
            "account": connected.then(|| get("account")).filter(|a| !a.is_empty()),
        })
    }

    /// Starts a sign-in: keeps the verifier and `state` (both random, from the caller) and the
    /// address to open. `client_id` blank keeps the one already saved.
    ///
    /// # Errors
    /// A sentence for the person: no client id yet, or the settings could not be saved.
    pub fn begin(
        &self,
        client_id: &str,
        redirect: &str,
        verifier: &str,
        state: &str,
        now_ms: i64,
    ) -> Result<Value, &'static str> {
        let client_id = client_id.trim();
        if !client_id.is_empty() {
            if client_id.len() > 64 || !client_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err("That is not a Spotify client id");
            }
            self.set("client_id", client_id)?;
        }
        let client_id = self.get("client_id");
        if client_id.is_empty() {
            return Err("Endora needs your Spotify app's client id first");
        }
        let url = sign_in_url(&client_id, &pkce_challenge(verifier), state, redirect);
        self.set("pending_state", state)?;
        self.set("pending_verifier", verifier)?;
        self.set("pending_redirect", redirect)?;
        self.set("pending_started_ms", &now_ms.to_string())?;
        self.set("pending_url", &url)?;
        Ok(self.status(now_ms))
    }

    /// Finishes the pending sign-in with what came back from Spotify (the address, its query, or
    /// a callback's query): swaps the code for tokens and keeps them. The pending sign-in is
    /// used up either way.
    ///
    /// # Errors
    /// A sentence for the person: nothing pending, the wrong sign-in, or Spotify refused.
    pub fn finish(&self, returned: &str, now_ms: i64) -> Result<Value, &'static str> {
        let all = self.all();
        let get = |k: &str| all.get(k).cloned().unwrap_or_default();
        let state = get("pending_state");
        let started = get("pending_started_ms").parse::<i64>().unwrap_or(0);
        if state.is_empty() || now_ms - started >= PENDING_FOR_MS {
            return Err("No sign-in is waiting; start it again");
        }
        let code = code_from(returned, &state)?;
        let (verifier, redirect, client_id) = (
            get("pending_verifier"),
            get("pending_redirect"),
            get("client_id"),
        );
        self.clear_pending()?;
        let value = self.accounts.token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("client_id", &client_id),
            ("code_verifier", &verifier),
        ])?;
        self.keep(&value, "", now_ms)?;
        Ok(self.status(now_ms))
    }

    /// Keeps the tokens Spotify gave, and who they belong to.
    fn keep(&self, value: &Value, old_refresh: &str, now_ms: i64) -> Result<String, &'static str> {
        let access = value["access_token"]
            .as_str()
            .filter(|a| !a.is_empty())
            .ok_or("Spotify gave no access token")?;
        // Spotify may hand back a new refresh token, and then only that one works.
        let refresh = value["refresh_token"].as_str().unwrap_or(old_refresh);
        if refresh.is_empty() {
            return Err("Spotify gave no refresh token");
        }
        let expires_in = value["expires_in"].as_i64().unwrap_or(3600);
        self.set("access_token", access)?;
        self.set("refresh_token", refresh)?;
        self.set(
            "access_expires_ms",
            &(now_ms + expires_in * 1000).to_string(),
        )?;
        if let Ok(me) = self.accounts.me(access) {
            let name = me["display_name"]
                .as_str()
                .or(me["id"].as_str())
                .unwrap_or("");
            self.set("account", &name.chars().take(80).collect::<String>())?;
        }
        Ok(access.to_owned())
    }

    /// Lends an access token and how many seconds it has left, refreshing it first when it is
    /// nearly spent. The refresh token is never returned.
    ///
    /// # Errors
    /// A sentence for the person: not connected, or Spotify refused the refresh.
    pub fn lend(&self, now_ms: i64) -> Result<(String, i64), &'static str> {
        let all = self.all();
        let get = |k: &str| all.get(k).cloned().unwrap_or_default();
        let refresh = get("refresh_token");
        if refresh.is_empty() {
            return Err("Spotify is not connected");
        }
        let expires = get("access_expires_ms").parse::<i64>().unwrap_or(0);
        let access = get("access_token");
        if !access.is_empty() && expires - now_ms > REFRESH_WITHIN_MS {
            return Ok((access, (expires - now_ms) / 1000));
        }
        let value = self.accounts.token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &get("client_id")),
        ])?;
        let access = self.keep(&value, &refresh, now_ms)?;
        let expires = self
            .get("access_expires_ms")
            .parse::<i64>()
            .unwrap_or(now_ms + 3_600_000);
        Ok((access, (expires - now_ms) / 1000))
    }

    /// Takes over a sign-in a device already has (the handheld's, from before Endora held it),
    /// proving it works by refreshing it once. The device then deletes its own copy.
    ///
    /// # Errors
    /// A sentence for the person: not a client id, or Spotify refused the token.
    pub fn take_over(
        &self,
        client_id: &str,
        refresh_token: &str,
        now_ms: i64,
    ) -> Result<Value, &'static str> {
        let client_id = client_id.trim();
        if client_id.is_empty()
            || client_id.len() > 64
            || !client_id.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err("That is not a Spotify client id");
        }
        let value = self.accounts.token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.trim()),
            ("client_id", client_id),
        ])?;
        self.set("client_id", client_id)?;
        self.keep(&value, refresh_token.trim(), now_ms)?;
        self.clear_pending()?;
        Ok(self.status(now_ms))
    }

    /// Forgets the sign-in on this node. Spotify itself still lists Endora under the account's
    /// apps until removed there.
    ///
    /// # Errors
    /// If the settings could not be saved.
    pub fn disconnect(&self, now_ms: i64) -> Result<Value, &'static str> {
        for key in [
            "refresh_token",
            "access_token",
            "access_expires_ms",
            "account",
        ] {
            self.set(key, "")?;
        }
        self.clear_pending()?;
        Ok(self.status(now_ms))
    }
}

#[cfg(test)]
mod tests;
