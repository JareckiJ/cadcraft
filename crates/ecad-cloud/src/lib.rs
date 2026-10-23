//! The community parts database, from CADCraft.
//!
//! - **Sign-in** ([`sign_in`]): Supabase Auth's GitHub login with PKCE, the flow for desktop
//!   apps: the browser opens Supabase's GitHub sign-in page, CADCraft listens on
//!   `http://127.0.0.1:53682/callback` for the result and exchanges the one-time code for a
//!   Supabase session. The session (its tokens) is kept in the OS credential store (Windows
//!   Credential Manager, macOS Keychain, the Linux kernel keyring), never in a file; the profile
//!   (GitHub login and id) is shown from it. Expired sessions are refreshed; a refused refresh
//!   asks the user to sign in again.
//! - **Parts** ([`search`], [`submit`]): the `manufacturer_parts` table through Supabase's REST
//!   API (PostgREST). Anyone may search; uploads carry the user's session token, and the server
//!   credits them to the user (see `cloud/supabase/migrations`).
//!
//! The app holds only the project URL and its public *anon* key ([`CloudConfig`]); the GitHub
//! client secret lives in Supabase. Desktop only: in the web build every call reports
//! [`CloudError::Unavailable`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use cadcraft_ecad::ManufacturerPart;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{current_session, open_in_browser, search, sign_in, sign_out, submit};

/// Where CADCraft waits for the sign-in result. Add it to Supabase's allowed redirect URLs.
pub const REDIRECT_PORT: u16 = 53682;
pub const REDIRECT_URL: &str = "http://127.0.0.1:53682/callback";

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("Set the community database's address and public key first.")]
    NotConfigured,
    #[error("Sign in with GitHub to share parts.")]
    NotSignedIn,
    #[error("Your sign-in has expired: please sign in with GitHub again.")]
    SessionExpired,
    #[error("Sign-in was cancelled.")]
    Cancelled,
    #[error("Sign-in timed out: nothing came back from the browser.")]
    TimedOut,
    #[error("The community database is not available in the web version.")]
    Unavailable,
    #[error("Could not reach the community database: {0}")]
    Network(String),
    #[error("{0}")]
    Server(String),
    #[error("{0}")]
    Invalid(String),
    #[error("The credential store could not be used: {0}")]
    Keyring(String),
}

pub type Result<T> = std::result::Result<T, CloudError>;

/// The Supabase project: its URL and public (anon) key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudConfig {
    /// `https://<project-ref>.supabase.co`
    pub url: String,
    /// The project's *anon public* key (never the service_role key).
    pub anon_key: String,
}

impl CloudConfig {
    /// The project URL without a trailing slash, checked: `https://` (or a local
    /// `http://127.0.0.1` / `http://localhost` test server), and a key that is not the
    /// service_role key.
    pub fn validated(&self) -> Result<(String, String)> {
        let url = self.url.trim().trim_end_matches('/').to_string();
        let key = self.anon_key.trim().to_string();
        if url.is_empty() || key.is_empty() {
            return Err(CloudError::NotConfigured);
        }
        let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
        if !url.starts_with("https://") && !local {
            return Err(CloudError::Invalid("The database address must start with https://".into()));
        }
        if jwt_role(&key).as_deref() == Some("service_role") {
            return Err(CloudError::Invalid("That is the service_role key: use the anon public key. Never share the service_role key.".into()));
        }
        Ok((url, key))
    }

    /// The settings file: `<config dir>/cadcraft/ecad-cloud.json`.
    pub fn path() -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            dirs::config_dir().map(|d| d.join("cadcraft").join("ecad-cloud.json"))
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// The saved settings, with `CADCRAFT_SUPABASE_URL` / `CADCRAFT_SUPABASE_ANON_KEY` taking
    /// precedence.
    pub fn load() -> CloudConfig {
        let mut c: CloudConfig = Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if let Ok(v) = std::env::var("CADCRAFT_SUPABASE_URL") {
            c.url = v;
        }
        if let Ok(v) = std::env::var("CADCRAFT_SUPABASE_ANON_KEY") {
            c.anon_key = v;
        }
        c
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path().ok_or(CloudError::Unavailable)?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CloudError::Invalid(e.to_string()))?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| CloudError::Invalid(e.to_string()))?;
        std::fs::write(&p, bytes).map_err(|e| CloudError::Invalid(format!("{}: {e}", p.display())))
    }
}

/// The `role` claim of a JWT (unverified: only to catch a pasted service_role key).
fn jwt_role(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64url_decode(payload)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("role")?.as_str().map(str::to_string)
}

/// Decode unpadded base64url (JWT segments), dependency-free so the web build has it too.
fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        })
    };
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        let take = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        out.extend(n.to_be_bytes().iter().skip(1).take(take));
    }
    Some(out)
}

/// A signed-in user, as shown in CADCraft.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    /// The Supabase user id (the database's `created_by`).
    pub user_id: String,
    pub github_login: String,
    /// GitHub's numeric user id.
    pub github_id: String,
    pub avatar_url: String,
}

/// A Supabase session (kept in the credential store).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: u64,
    pub profile: Profile,
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;

    pub fn sign_in(_c: &CloudConfig, _cancel: &std::sync::atomic::AtomicBool, _open: impl FnOnce(&str)) -> Result<Session> {
        Err(CloudError::Unavailable)
    }
    pub fn current_session(_c: &CloudConfig) -> Result<Option<Session>> {
        Ok(None)
    }
    pub fn sign_out() -> Result<()> {
        Ok(())
    }
    pub fn search(_c: &CloudConfig, _q: &str, _limit: usize) -> Result<Vec<ManufacturerPart>> {
        Err(CloudError::Unavailable)
    }
    pub fn submit(_c: &CloudConfig, _p: &ManufacturerPart) -> Result<ManufacturerPart> {
        Err(CloudError::Unavailable)
    }
    pub fn open_in_browser(_url: &str) -> Result<()> {
        Err(CloudError::Unavailable)
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::{current_session, open_in_browser, search, sign_in, sign_out, submit};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_checks() {
        let c = |url: &str, key: &str| CloudConfig { url: url.into(), anon_key: key.into() };
        assert!(matches!(c("", "").validated(), Err(CloudError::NotConfigured)));
        assert!(c("http://example.com", "k").validated().is_err());
        assert_eq!(c("https://abc.supabase.co/", "k").validated().unwrap().0, "https://abc.supabase.co");
        assert!(c("http://127.0.0.1:54321", "k").validated().is_ok());
        // A JWT whose role is service_role is refused: {"role":"service_role"}.
        let service = format!("x.{}.y", "eyJyb2xlIjoic2VydmljZV9yb2xlIn0");
        assert!(c("https://abc.supabase.co", &service).validated().is_err());
        let anon = format!("x.{}.y", "eyJyb2xlIjoiYW5vbiJ9");
        assert!(c("https://abc.supabase.co", &anon).validated().is_ok());
    }

    #[test]
    fn base64url() {
        assert_eq!(base64url_decode("eyJyb2xlIjoiYW5vbiJ9").as_deref(), Some(&br#"{"role":"anon"}"#[..]));
        assert_eq!(base64url_decode("TWE").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(base64url_decode("!!"), None);
    }
}
