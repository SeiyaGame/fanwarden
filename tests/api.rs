//! HTTP API tests against the real router (no network, no hardware).

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use fanwarden::bmc::Bmc;
use fanwarden::calibration::CalibrationStatus;
use fanwarden::config::{Config, FanMode, NotificationChannel, SECRET_PLACEHOLDER};
use fanwarden::events::EventLog;
use fanwarden::sim::Simulator;
use fanwarden::state::{AppState, Snapshot};
use fanwarden::web;

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
async fn reload_config_from_file() {
    let mut app = logged_in(Config::default()).await;
    let path = app.state.config_path.clone();

    // Edited by hand: applied on reload, the password hash written by setup is kept.
    let text = std::fs::read_to_string(&path).unwrap().replace("interval_secs = 10", "interval_secs = 42");
    std::fs::write(&path, text).unwrap();
    let (status, body, _) = app.call("POST", "/api/config/reload", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["general"]["interval_secs"], 42);
    assert_eq!(app.state.config().general.interval_secs, 42);
    assert!(app.state.config().web.password_hash.starts_with("$argon2"));

    // Broken file: error, running configuration untouched.
    std::fs::write(
        &path,
        "[general]
interval_secs = \"oops\"
",
    )
    .unwrap();
    let (status, body, _) = app.call("POST", "/api/config/reload", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("invalid configuration"));
    assert_eq!(app.state.config().general.interval_secs, 42);
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
async fn config_history_and_restore() {
    let mut app = logged_in(Config::default()).await;
    let hash = app.state.config().web.password_hash;

    let (_, mut config, _) = app.call("GET", "/api/config", None).await;
    config["general"]["interval_secs"] = json!(30);
    assert_eq!(app.call("PUT", "/api/config", Some(config)).await.0, StatusCode::OK);

    let (status, versions, _) = app.call("GET", "/api/config/history", None).await;
    assert_eq!(status, StatusCode::OK);
    let versions = versions.as_array().unwrap();
    // Startup, then the save (the password setup alone is not a version).
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["origin"]["kind"], "ui");
    assert!(versions[0]["toml"].as_str().unwrap().contains("interval_secs = 30"));
    let first = versions[1]["id"].as_u64().unwrap();

    let (status, _, _) = app.call("POST", &format!("/api/config/history/{first}/restore"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.state.config().general.interval_secs, 10);
    // Restoring never brings back an old web password.
    assert_eq!(app.state.config().web.password_hash, hash);
    let (_, versions, _) = app.call("GET", "/api/config/history", None).await;
    assert_eq!(versions[0]["origin"]["kind"], "restore");

    assert_eq!(app.call("POST", "/api/config/history/999/restore", None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn calibration_starts_once_and_can_be_cancelled() {
    let mut app = logged_in(Config::default()).await;
    assert_eq!(app.call("POST", "/api/zones/1/calibration", None).await.0, StatusCode::SERVICE_UNAVAILABLE);

    let sim = Arc::new(Mutex::new(Simulator::default()));
    sim.lock().unwrap().mode = FanMode::Full;
    *app.state.bmc.write().unwrap() = Some(Bmc::Sim(sim));
    assert_eq!(app.call("POST", "/api/zones/9/calibration", None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.call("POST", "/api/zones/1/calibration", None).await.0, StatusCode::OK);
    assert_eq!(app.call("POST", "/api/zones/0/calibration", None).await.0, StatusCode::CONFLICT);
    assert_eq!(app.call("DELETE", "/api/zones/0/calibration", None).await.0, StatusCode::CONFLICT);
    assert_eq!(app.call("DELETE", "/api/zones/1/calibration", None).await.0, StatusCode::OK);

    let mut status = None;
    for _ in 0..50 {
        status = app.state.calibration();
        if status.as_ref().is_some_and(|s| s.running_zone().is_none()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status, Some(CalibrationStatus::Failed { zone: 1, reason: "cancelled".into() }));
}

#[tokio::test]
async fn bmc_event_log() {
    let mut app = logged_in(Config::default()).await;
    let (status, body, _) = app.call("GET", "/api/bmc/events", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"], json!([]));

    *app.state.bmc.write().unwrap() = Some(Bmc::Sim(Arc::new(Mutex::new(Simulator::default()))));
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(fanwarden::sel::task(app.state.clone(), shutdown));
    for _ in 0..50 {
        if app.state.bmc_log().updated.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    stop.send(true).unwrap();
    task.await.unwrap();

    let (_, body, _) = app.call("GET", "/api/bmc/events", None).await;
    assert_eq!(body["percent_used"], 2);
    assert_eq!(body["entries"].as_array().unwrap().len(), 5);
    assert_eq!(body["entries"][3]["event"], "Correctable ECC");
    assert_eq!(body["entries"][3]["severity"], "warning");
}

#[tokio::test]
async fn calibration_is_refused_in_dry_run() {
    let mut config = Config::default();
    config.general.dry_run = true;
    let mut app = logged_in(config).await;
    *app.state.bmc.write().unwrap() = Some(Bmc::Sim(Arc::new(Mutex::new(Simulator::default()))));
    assert_eq!(app.call("POST", "/api/zones/0/calibration", None).await.0, StatusCode::CONFLICT);
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
    assert!(body.as_str().unwrap().contains("fanwarden_cpu_temperature 42"));

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
