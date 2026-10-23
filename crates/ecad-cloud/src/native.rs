//! The desktop implementation: HTTPS with `ureq`, the loopback sign-in listener, the keyring.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{CloudConfig, CloudError, ManufacturerPart, Profile, REDIRECT_PORT, REDIRECT_URL, Result, Session};

const KEYRING_SERVICE: &str = "cadcraft-ecad-community";
const KEYRING_USER: &str = "supabase-session";
/// How long sign-in waits for the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// Refresh a session this close to expiry.
const REFRESH_MARGIN: u64 = 60;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A request error as the user should see it: the server's message when it gave one.
fn http_err(e: ureq::Error) -> CloudError {
    match e {
        ureq::Error::Status(code, resp) => {
            let body: Value = resp.into_json().unwrap_or(Value::Null);
            let msg = ["message", "msg", "error_description", "error"]
                .iter()
                .find_map(|k| body.get(*k).and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| format!("the server answered {code}"));
            if code == 401 || msg.contains("JWT expired") { CloudError::SessionExpired } else { CloudError::Server(msg) }
        }
        ureq::Error::Transport(t) => CloudError::Network(t.to_string()),
    }
}

// ---------------- the keyring ----------------

fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| CloudError::Keyring(e.to_string()))
}

fn store_session(s: &Session) -> Result<()> {
    let json = serde_json::to_string(s).map_err(|e| CloudError::Invalid(e.to_string()))?;
    entry()?.set_password(&json).map_err(|e| CloudError::Keyring(e.to_string()))
}

fn load_session() -> Result<Option<Session>> {
    match entry()?.get_password() {
        Ok(j) => Ok(serde_json::from_str(&j).ok()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CloudError::Keyring(e.to_string())),
    }
}

/// Forget the session.
pub fn sign_out() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CloudError::Keyring(e.to_string())),
    }
}

// ---------------- sessions ----------------

/// A session from Supabase's token response.
fn session_from(v: &Value) -> Result<Session> {
    let s = |p: &str| v.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
    let access = s("/access_token");
    if access.is_empty() {
        return Err(CloudError::Server("The sign-in answer had no session.".into()));
    }
    let expires_at =
        v.get("expires_at").and_then(Value::as_u64).unwrap_or_else(|| now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600));
    let meta = |k: &str| v.pointer(&format!("/user/user_metadata/{k}")).and_then(Value::as_str).unwrap_or("").to_string();
    Ok(Session {
        access_token: access,
        refresh_token: s("/refresh_token"),
        expires_at,
        profile: Profile {
            user_id: s("/user/id"),
            github_login: [meta("user_name"), meta("preferred_username")].into_iter().find(|x| !x.is_empty()).unwrap_or_default(),
            github_id: [meta("provider_id"), meta("sub")].into_iter().find(|x| !x.is_empty()).unwrap_or_default(),
            avatar_url: meta("avatar_url"),
        },
    })
}

fn refresh(c: &CloudConfig, old: &Session) -> Result<Session> {
    let (url, key) = c.validated()?;
    let r = agent()
        .post(&format!("{url}/auth/v1/token?grant_type=refresh_token"))
        .set("apikey", &key)
        .send_json(json!({ "refresh_token": old.refresh_token }));
    match r {
        Ok(resp) => {
            let v: Value = resp.into_json().map_err(|e| CloudError::Network(e.to_string()))?;
            let mut s = session_from(&v)?;
            if s.profile.github_login.is_empty() {
                s.profile = old.profile.clone();
            }
            store_session(&s)?;
            Ok(s)
        }
        // A refused refresh token: the user signs in again.
        Err(ureq::Error::Status(400 | 401 | 403, _)) => {
            sign_out()?;
            Err(CloudError::SessionExpired)
        }
        Err(e) => Err(http_err(e)),
    }
}

/// The signed-in session, refreshed if it is about to expire; `None` when signed out.
pub fn current_session(c: &CloudConfig) -> Result<Option<Session>> {
    let Some(s) = load_session()? else { return Ok(None) };
    if s.expires_at > now() + REFRESH_MARGIN {
        return Ok(Some(s));
    }
    refresh(c, &s).map(Some)
}

// ---------------- signing in ----------------

fn pkce_pair() -> Result<(String, String)> {
    let mut raw = [0u8; 48];
    getrandom::getrandom(&mut raw).map_err(|e| CloudError::Invalid(e.to_string()))?;
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Ok((verifier, challenge))
}

pub(crate) fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The value of `name` in the query of a request line (`GET /callback?code=…&x=y HTTP/1.1`).
fn query_param(request_line: &str, name: &str) -> Option<String> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == name).then(|| decode_component(v))
    })
}

fn decode_component(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'%' => {
                let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(c),
                }
            }
            b'+' => out.push(b' '),
            _ => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Wait on the loopback port for the browser's callback: the authorisation code.
fn wait_for_code(listener: &TcpListener, cancel: &AtomicBool) -> Result<String> {
    listener.set_nonblocking(true).map_err(|e| CloudError::Network(e.to_string()))?;
    let deadline = Instant::now() + SIGN_IN_TIMEOUT;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CloudError::Cancelled);
        }
        if Instant::now() > deadline {
            return Err(CloudError::TimedOut);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut line = String::new();
                let _ = BufReader::new(&stream).take_line(&mut line);
                if !line.starts_with("GET /callback") {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let (title, result) = match (query_param(&line, "code"), query_param(&line, "error_description")) {
                    (Some(code), _) => ("Signed in", Ok(code)),
                    (None, Some(err)) => ("Sign-in failed", Err(CloudError::Server(err))),
                    (None, None) => ("Sign-in failed", Err(CloudError::Server("GitHub sign-in returned no code.".into()))),
                };
                let page = format!(
                    "<!doctype html><meta charset=utf-8><title>CADCraft</title><body style=\"font-family:sans-serif;padding:3em\">\
                     <h2>{title}</h2><p>You can close this tab and return to CADCraft.</p></body>"
                );
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                );
                return result;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(CloudError::Network(e.to_string())),
        }
    }
}

trait TakeLine {
    fn take_line(&mut self, line: &mut String) -> std::io::Result<usize>;
}

impl<R: BufRead> TakeLine for R {
    /// One request line, at most 8 kB.
    fn take_line(&mut self, line: &mut String) -> std::io::Result<usize> {
        let mut buf = Vec::new();
        let n = std::io::Read::take(self, 8192).read_until(b'\n', &mut buf)?;
        *line = String::from_utf8_lossy(&buf).to_string();
        Ok(n)
    }
}

/// Sign in with GitHub through Supabase. `open` shows the sign-in page (the app opens the
/// browser; it receives the URL so it can also show it); `cancel` stops waiting. Blocks until
/// the browser comes back, so call it from a background thread.
pub fn sign_in(c: &CloudConfig, cancel: &AtomicBool, open: impl FnOnce(&str)) -> Result<Session> {
    let (url, key) = c.validated()?;
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .map_err(|e| CloudError::Network(format!("port {REDIRECT_PORT} is busy ({e}); close other sign-ins and try again")))?;
    let (verifier, challenge) = pkce_pair()?;
    let page =
        format!("{url}/auth/v1/authorize?provider=github&redirect_to={}&code_challenge={challenge}&code_challenge_method=s256", encode(REDIRECT_URL));
    open(&page);
    let code = wait_for_code(&listener, cancel)?;
    let resp = agent()
        .post(&format!("{url}/auth/v1/token?grant_type=pkce"))
        .set("apikey", &key)
        .send_json(json!({ "auth_code": code, "code_verifier": verifier }))
        .map_err(http_err)?;
    let v: Value = resp.into_json().map_err(|e| CloudError::Network(e.to_string()))?;
    let s = session_from(&v)?;
    store_session(&s)?;
    Ok(s)
}

// ---------------- parts ----------------

/// Search the community parts: every word of `query` in the part number, manufacturer or type.
pub fn search(c: &CloudConfig, query: &str, limit: usize) -> Result<Vec<ManufacturerPart>> {
    let (url, key) = c.validated()?;
    // PostgREST's or=(…) syntax: keep only characters that cannot break it.
    let clean: String = query.chars().filter(|ch| ch.is_alphanumeric() || " -_./".contains(*ch)).collect();
    let mut q = format!("{url}/rest/v1/manufacturer_parts?select=*&order=updated_at.desc&limit={}", limit.clamp(1, 200));
    for w in clean.split_whitespace().take(5) {
        let pat = encode(&format!("*{w}*"));
        q.push_str(&format!("&or=(part_number.ilike.{pat},manufacturer.ilike.{pat},component_type.ilike.{pat},description.ilike.{pat})"));
    }
    let resp = agent().get(&q).set("apikey", &key).set("Authorization", &format!("Bearer {key}")).call().map_err(http_err)?;
    resp.into_json().map_err(|e| CloudError::Server(format!("Unexpected answer: {e}")))
}

/// Share `part` as the signed-in user. The server credits it to them and enforces the upload
/// limit; the stored record comes back.
pub fn submit(c: &CloudConfig, part: &ManufacturerPart) -> Result<ManufacturerPart> {
    let (url, key) = c.validated()?;
    part.validate().map_err(|e| CloudError::Invalid(e.to_string()))?;
    let session = current_session(c)?.ok_or(CloudError::NotSignedIn)?;
    // Only the content columns: id, credit and moderation are the server's.
    let mut body = serde_json::to_value(part).map_err(|e| CloudError::Invalid(e.to_string()))?;
    if let Some(o) = body.as_object_mut() {
        for k in ["id", "created_by", "github_login", "updated_at", "status", "report_count", "created_at"] {
            o.remove(k);
        }
        o.insert("license".into(), json!(cadcraft_ecad::PART_LICENSE));
    }
    let resp = agent()
        .post(&format!("{url}/rest/v1/manufacturer_parts"))
        .set("apikey", &key)
        .set("Authorization", &format!("Bearer {}", session.access_token))
        .set("Prefer", "return=representation")
        .send_json(body)
        .map_err(|e| match http_err(e) {
            CloudError::Server(m) if m.contains("duplicate key") => {
                CloudError::Server(format!("{} {} is already in the community database.", part.manufacturer, part.part_number))
            }
            other => other,
        })?;
    let rows: Vec<ManufacturerPart> = resp.into_json().map_err(|e| CloudError::Server(format!("Unexpected answer: {e}")))?;
    rows.into_iter().next().ok_or_else(|| CloudError::Server("The part was not stored.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_the_sha256_of_the_verifier() {
        let (v, c) = pkce_pair().unwrap();
        assert!(v.len() >= 43 && v.len() <= 128, "RFC 7636 length");
        assert_eq!(c, base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes())));
        assert_ne!(pkce_pair().unwrap().0, v);
    }

    #[test]
    fn callback_parsing() {
        let line = "GET /callback?code=ab%2Fc+d&state=x HTTP/1.1\r\n";
        assert_eq!(query_param(line, "code").as_deref(), Some("ab/c d"));
        assert_eq!(query_param(line, "state").as_deref(), Some("x"));
        assert_eq!(query_param(line, "nope"), None);
        assert_eq!(query_param("GET /callback HTTP/1.1", "code"), None);
        assert_eq!(encode("http://127.0.0.1:53682/callback"), "http%3A%2F%2F127.0.0.1%3A53682%2Fcallback");
        assert_eq!(decode_component("%zz%4"), "%zz%4");
    }

    #[test]
    fn sessions_from_supabase_answers() {
        let v = json!({
            "access_token": "a", "refresh_token": "r", "expires_in": 3600,
            "user": { "id": "u-1", "user_metadata": { "user_name": "elliott", "provider_id": "42", "avatar_url": "https://x/a.png" } }
        });
        let s = session_from(&v).unwrap();
        assert_eq!((s.profile.github_login.as_str(), s.profile.github_id.as_str(), s.profile.user_id.as_str()), ("elliott", "42", "u-1"));
        assert!(s.expires_at > now());
        assert!(session_from(&json!({})).is_err());
    }

    #[test]
    fn the_loopback_listener_returns_the_code() {
        // A browser's callback, sent to a listener on a free port.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut out);
            out
        });
        let cancel = AtomicBool::new(false);
        assert_eq!(wait_for_code(&listener, &cancel).unwrap(), "xyz");
        assert!(t.join().unwrap().contains("Signed in"));
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(wait_for_code(&listener, &cancel), Err(CloudError::Cancelled)));
    }

    #[test]
    fn unconfigured_calls_explain_themselves() {
        let c = CloudConfig::default();
        assert!(matches!(search(&c, "relay", 10), Err(CloudError::NotConfigured)));
        assert!(matches!(submit(&c, &ManufacturerPart::default()), Err(CloudError::NotConfigured)));
    }
}
