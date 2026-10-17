//! Web UI password (Argon2) and cookie sessions.

use std::collections::HashMap;
use std::sync::Mutex;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use std::sync::Arc;

use crate::state::AppState;

pub const COOKIE: &str = "fan_control_session";
/// Failed logins tolerated per minute before refusing every attempt.
const MAX_FAILURES_PER_MINUTE: u32 = 10;
pub const MIN_PASSWORD_LEN: usize = 8;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow::anyhow!("cannot hash password: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

#[derive(Default)]
pub struct Sessions {
    /// Token → expiry (unix seconds).
    tokens: Mutex<HashMap<String, u64>>,
    /// (minute, failures during that minute).
    failures: Mutex<(u64, u32)>,
}

impl Sessions {
    pub fn create(&self, hours: u64) -> String {
        let bytes: [u8; 32] = rand::random();
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let now = crate::now();
        let mut tokens = self.tokens.lock().unwrap();
        tokens.retain(|_, expiry| *expiry > now);
        tokens.insert(token.clone(), now + hours.max(1) * 3600);
        token
    }

    pub fn is_valid(&self, token: &str) -> bool {
        self.tokens.lock().unwrap().get(token).is_some_and(|expiry| *expiry > crate::now())
    }

    pub fn remove(&self, token: &str) {
        self.tokens.lock().unwrap().remove(token);
    }

    /// Logs every session out (password change).
    pub fn clear(&self) {
        self.tokens.lock().unwrap().clear();
    }

    pub fn too_many_failures(&self) -> bool {
        let (minute, count) = *self.failures.lock().unwrap();
        minute == crate::now() / 60 && count >= MAX_FAILURES_PER_MINUTE
    }

    pub fn record_failure(&self) {
        let minute = crate::now() / 60;
        let mut failures = self.failures.lock().unwrap();
        if failures.0 != minute {
            *failures = (minute, 0);
        }
        failures.1 += 1;
    }
}

pub fn session_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_string())
}

pub fn session_cookie(token: &str, hours: u64) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}", hours * 3600)
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// Rejects API calls without a valid session.
pub async fn require_session(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let authorized = session_token(request.headers()).is_some_and(|t| state.sessions.is_valid(&t));
    if authorized {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "authentication required").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password(&hash, "correct horse"));
        assert!(!verify_password(&hash, "wrong"));
        assert!(!verify_password("", "anything"));
    }

    #[test]
    fn sessions() {
        let s = Sessions::default();
        let token = s.create(1);
        assert!(s.is_valid(&token));
        assert!(!s.is_valid("nope"));
        s.remove(&token);
        assert!(!s.is_valid(&token));
    }

    #[test]
    fn cookie_parsing() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::COOKIE, format!("a=b; {COOKIE}=tok123; c=d").parse().unwrap());
        assert_eq!(session_token(&headers).as_deref(), Some("tok123"));
    }

    #[test]
    fn rate_limit() {
        let s = Sessions::default();
        for _ in 0..MAX_FAILURES_PER_MINUTE {
            assert!(!s.too_many_failures());
            s.record_failure();
        }
        assert!(s.too_many_failures());
    }
}
