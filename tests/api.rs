//! HTTP API tests against the real router (no network, no hardware).

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use fan_control::config::{Config, NotificationChannel, SECRET_PLACEHOLDER};
use fan_control::events::EventLog;
use fan_control::state::{AppState, Snapshot};
use fan_control::web;

struct App {
    router: Router,
    state: Arc<AppState>,
    cookie: Option<String>,
    _dir: tempfile::TempDir,
}

impl App {
    fn new(config: Config) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        config.save(&path).unwrap();
        let state = AppState::new(path, config, None, EventLog::default(), true, false);
        Self { router: web::router(state.clone()), state, cookie: None, _dir: dir }
    }

    async fn call(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value, Option<String>) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let request = match body {
            Some(body) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value =
            serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
        if let Some(cookie) = &set_cookie {
            self.cookie = Some(cookie.clone());
        }
        (status, value, set_cookie)
    }
}

#[tokio::test]
async fn first_setup_then_login() {
    let mut app = App::new(Config::default());

    let (_, body, _) = app.call("GET", "/api/auth/status", None).await;
    assert_eq!(body, json!({ "setup_required": true, "authenticated": false }));
    assert_eq!(app.call("GET", "/api/status", None).await.0, StatusCode::UNAUTHORIZED);

    // Too short.
    let (status, _, _) = app.call("POST", "/api/auth/setup", Some(json!({ "password": "short" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _, cookie) = app.call("POST", "/api/auth/setup", Some(json!({ "password": "long enough" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_some());
    assert_eq!(app.call("GET", "/api/status", None).await.0, StatusCode::OK);

    // Setup cannot be replayed once a password exists.
    let (status, _, _) = app.call("POST", "/api/auth/setup", Some(json!({ "password": "attacker pass" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // The hash is persisted to disk.
    let saved = Config::load_or_create(&app.state.config_path).unwrap();
    assert!(saved.web.password_hash.starts_with("$argon2"));

    app.call("POST", "/api/auth/logout", None).await;
    assert_eq!(app.call("GET", "/api/status", None).await.0, StatusCode::UNAUTHORIZED);

    let (status, _, _) = app.call("POST", "/api/auth/login", Some(json!({ "password": "wrong password" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.call("POST", "/api/auth/login", Some(json!({ "password": "long enough" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.call("GET", "/api/status", None).await.0, StatusCode::OK);
}

async fn logged_in(config: Config) -> App {
    let mut app = App::new(config);
    let (status, _, _) = app.call("POST", "/api/auth/setup", Some(json!({ "password": "long enough" }))).await;
    assert_eq!(status, StatusCode::OK);
    app
}

#[tokio::test]
async fn config_secrets_never_leave_the_server() {
    let mut config = Config::default();
    config.ipmi.password = "bmc-secret".into();
    config.notifications.push(NotificationChannel::Discord { enabled: true, url: "https://discord.test/hook".into() });
    let mut app = logged_in(config).await;

    let (status, body, _) = app.call("GET", "/api/config", None).await;
    assert_eq!(status, StatusCode::OK);
    let text = body.to_string();
    assert!(!text.contains("bmc-secret") && !text.contains("discord.test") && !text.contains("argon2"));
    assert_eq!(body["ipmi"]["password"], SECRET_PLACEHOLDER);

    // Send it back with one change: secrets are kept.
    let mut edited = body.clone();
    edited["general"]["interval_secs"] = json!(15);
    let (status, _, _) = app.call("PUT", "/api/config", Some(edited)).await;
    assert_eq!(status, StatusCode::OK);
    let saved = app.state.config();
    assert_eq!(saved.general.interval_secs, 15);
    assert_eq!(saved.ipmi.password, "bmc-secret");
    assert!(saved.web.password_hash.starts_with("$argon2"));
    assert!(
        matches!(&saved.notifications[0], NotificationChannel::Discord { url, .. } if url == "https://discord.test/hook")
    );
}

#[tokio::test]
async fn invalid_config_is_rejected() {
    let mut app = logged_in(Config::default()).await;
    let (_, mut body, _) = app.call("GET", "/api/config", None).await;
    body["zones"][0]["rules"][0]["curve"] = json!([[50.0, 50], [40.0, 60]]);
    let (status, error, _) = app.call("PUT", "/api/config", Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("increasing"));
    assert_eq!(app.state.config(), Config::load_or_create(&app.state.config_path).unwrap());
}

#[tokio::test]
async fn manual_duty() {
    let mut app = logged_in(Config::default()).await;
    let (status, _, _) = app.call("PUT", "/api/zones/1/manual", Some(json!({ "duty": 80 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.state.manual.lock().unwrap().get(&1), Some(&80));

    assert_eq!(app.call("PUT", "/api/zones/1/manual", Some(json!({ "duty": 101 }))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.call("PUT", "/api/zones/9/manual", Some(json!({ "duty": 50 }))).await.0, StatusCode::NOT_FOUND);

    app.call("PUT", "/api/zones/1/manual", Some(json!({ "duty": null }))).await;
    assert!(app.state.manual.lock().unwrap().is_empty());
}

#[tokio::test]
async fn metrics_are_public_and_can_be_disabled() {
    let mut config = Config::default();
    let mut app = App::new(config.clone());
    let mut snapshot = Snapshot::default();
    snapshot.sensors.cpu = Some(42.0);
    app.state.publish(snapshot);

    let (status, body, _) = app.call("GET", "/metrics", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_str().unwrap().contains("fan_control_supermicro_cpu_temperature 42"));

    config.web.metrics_enabled = false;
    app.state.set_config(config).unwrap();
    assert_eq!(app.call("GET", "/metrics", None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ui_is_served_with_spa_fallback() {
    let mut app = App::new(Config::default());
    let (status, body, _) = app.call("GET", "/zones", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_str().unwrap().contains("<"));
    assert_eq!(app.call("GET", "/api/nope", None).await.0, StatusCode::NOT_FOUND);
}
