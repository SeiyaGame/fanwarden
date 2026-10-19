//! JSON API used by the web UI.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use super::auth;
use crate::calibration::{self, CalibrationStatus, Timing};
use crate::config::{Config, NotificationChannel};
use crate::config_history::Origin;
use crate::notify::{Notification, Severity};
use crate::state::AppState;

type ApiResult = Result<Response, ApiError>;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn bad_request(message: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.to_string())
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

// ---- Authentication ------------------------------------------------------------------------

pub async fn auth_status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Json<serde_json::Value> {
    let setup_required = state.config().web.password_hash.is_empty();
    let authenticated = auth::session_token(&headers).is_some_and(|t| state.sessions.is_valid(&t));
    Json(json!({ "setup_required": setup_required, "authenticated": authenticated }))
}

#[derive(Deserialize)]
pub struct PasswordBody {
    password: String,
}

fn login_response(state: &AppState, hours: u64) -> Response {
    let token = state.sessions.create(hours);
    ([(header::SET_COOKIE, auth::session_cookie(&token, hours))], Json(json!({ "ok": true }))).into_response()
}

/// First start: choose the password. Only allowed while none is set.
pub async fn setup(State(state): State<Arc<AppState>>, Json(body): Json<PasswordBody>) -> ApiResult {
    let mut config = state.config();
    if !config.web.password_hash.is_empty() {
        return Err(ApiError(StatusCode::CONFLICT, "a password is already set".into()));
    }
    if body.password.chars().count() < auth::MIN_PASSWORD_LEN {
        return Err(bad_request(format!("the password needs at least {} characters", auth::MIN_PASSWORD_LEN)));
    }
    config.web.password_hash = auth::hash_password(&body.password)?;
    state.set_config(config.clone())?;
    tracing::info!("web UI password created");
    Ok(login_response(&state, config.web.session_hours))
}

pub async fn login(State(state): State<Arc<AppState>>, Json(body): Json<PasswordBody>) -> ApiResult {
    if state.sessions.too_many_failures() {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many attempts, wait a minute".into()));
    }
    let config = state.config();
    let hash = config.web.password_hash.clone();
    let password = body.password;
    let valid = tokio::task::spawn_blocking(move || auth::verify_password(&hash, &password)).await.unwrap_or(false);
    if !valid {
        state.sessions.record_failure();
        tracing::warn!("failed login on the web UI");
        // Slows down guessing.
        tokio::time::sleep(Duration::from_millis(500)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong password".into()));
    }
    Ok(login_response(&state, config.web.session_hours))
}

pub async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(token) = auth::session_token(&headers) {
        state.sessions.remove(&token);
    }
    ([(header::SET_COOKIE, auth::clear_cookie())], Json(json!({ "ok": true }))).into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    current: String,
    new: String,
}

pub async fn change_password(State(state): State<Arc<AppState>>, Json(body): Json<ChangePasswordBody>) -> ApiResult {
    let mut config = state.config();
    if !auth::verify_password(&config.web.password_hash, &body.current) {
        return Err(ApiError(StatusCode::FORBIDDEN, "the current password is wrong".into()));
    }
    if body.new.chars().count() < auth::MIN_PASSWORD_LEN {
        return Err(bad_request(format!("the password needs at least {} characters", auth::MIN_PASSWORD_LEN)));
    }
    config.web.password_hash = auth::hash_password(&body.new)?;
    state.set_config(config.clone())?;
    state.sessions.clear();
    tracing::info!("web UI password changed");
    Ok(login_response(&state, config.web.session_hours))
}

// ---- Status --------------------------------------------------------------------------------

pub async fn status(State(state): State<Arc<AppState>>) -> Response {
    Json(state.snapshot()).into_response()
}

pub async fn live(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| stream_snapshots(state, socket))
}

async fn stream_snapshots(state: Arc<AppState>, mut socket: WebSocket) {
    let mut updates = state.live.subscribe();
    let send = |s: &crate::state::Snapshot| serde_json::to_string(s).map(|t| Message::Text(t.into()));
    if let Ok(message) = send(&state.snapshot())
        && socket.send(message).await.is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(snapshot) => {
                    let Ok(message) = send(&snapshot) else { continue };
                    if socket.send(message).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}

#[derive(Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    after: u64,
}

pub async fn events(State(state): State<Arc<AppState>>, Query(q): Query<EventsQuery>) -> Response {
    Json(state.events.since(q.after)).into_response()
}

// ---- Configuration -------------------------------------------------------------------------

pub async fn get_config(State(state): State<Arc<AppState>>) -> Response {
    Json(state.config().redacted()).into_response()
}

pub async fn put_config(State(state): State<Arc<AppState>>, Json(incoming): Json<Config>) -> ApiResult {
    let current = state.config();
    let merged = current.merge_from_ui(incoming);
    merged.validate().map_err(|e| bad_request(format!("{e:#}")))?;
    if merged.web.listen != current.web.listen {
        tracing::info!("web listen address changed to {}: restart the service to apply", merged.web.listen);
    }
    state.set_config(merged)?;
    state.config_history.record(&state.config(), Origin::Ui, crate::now());
    tracing::info!("configuration updated from the web UI");
    Ok(Json(state.config().redacted()).into_response())
}

pub async fn reload_config(State(state): State<Arc<AppState>>) -> ApiResult {
    let listen = state.config().web.listen;
    let config = state.reload_config().map_err(|e| {
        tracing::warn!("configuration file not reloaded: {e:#}");
        bad_request(format!("{e:#}"))
    })?;
    if config.web.listen != listen {
        tracing::info!("web listen address changed to {}: restart the service to apply", config.web.listen);
    }
    state.config_history.record(&config, Origin::File, crate::now());
    tracing::info!("configuration reloaded from {}", state.config_path.display());
    Ok(Json(config.redacted()).into_response())
}

/// Last versions of the configuration, newest first.
pub async fn config_history(State(state): State<Arc<AppState>>) -> Response {
    Json(state.config_history.views()).into_response()
}

/// Brings a previous version back. The web password stays the current one.
pub async fn restore_config(State(state): State<Arc<AppState>>, Path(id): Path<u64>) -> ApiResult {
    let Some(version) = state.config_history.get(id) else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("unknown version {id}")));
    };
    let mut config = version.config;
    config.web.password_hash = state.config().web.password_hash;
    config.validate().map_err(|e| bad_request(format!("{e:#}")))?;
    state.set_config(config)?;
    state.config_history.record(&state.config(), Origin::Restore { from_ts: version.ts }, crate::now());
    tracing::info!("configuration restored to version {id}");
    Ok(Json(state.config().redacted()).into_response())
}

// ---- Manual control ------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ManualBody {
    /// `null` goes back to automatic control.
    duty: Option<u8>,
}

pub async fn set_manual(
    State(state): State<Arc<AppState>>,
    Path(zone): Path<u8>,
    Json(body): Json<ManualBody>,
) -> ApiResult {
    let config = state.config();
    let Some(z) = config.zones.iter().find(|z| z.id == zone) else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("unknown zone {zone}")));
    };
    match body.duty {
        Some(duty) if duty > 100 => return Err(bad_request("duty must be between 0 and 100")),
        Some(duty) => {
            state.manual.lock().unwrap().insert(zone, duty);
            tracing::info!("zone {}: manual duty {duty}%", z.name);
        }
        None => {
            state.manual.lock().unwrap().remove(&zone);
            tracing::info!("zone {}: back to automatic", z.name);
        }
    }
    state.wake.notify_one();
    Ok(Json(json!({ "ok": true })).into_response())
}

// ---- Calibration ---------------------------------------------------------------------------

/// Starts sweeping a zone; progress and outcome come through the live snapshot.
pub async fn start_calibration(State(state): State<Arc<AppState>>, Path(zone): Path<u8>) -> ApiResult {
    let config = state.config();
    let Some(name) = config.zones.iter().find(|z| z.id == zone).map(|z| z.name.clone()) else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("unknown zone {zone}")));
    };
    if !matches!(zone, 0 | 1) {
        return Err(bad_request("only zones 0 (FAN1…) and 1 (FANA…) have known fans"));
    }
    if state.dry_run(&config) {
        return Err(ApiError(StatusCode::CONFLICT, "dry-run: nothing is written to the BMC".into()));
    }
    let Some(bmc) = state.bmc.read().unwrap().clone() else {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "the control loop has not started yet".into()));
    };
    if !state.try_start_calibration(zone) {
        return Err(ApiError(StatusCode::CONFLICT, "a calibration is already running".into()));
    }
    // Simulated fans react at once: no need to wait for them.
    let timing = if state.simulate {
        Timing { spin_up: Duration::from_secs(2), settle: Duration::from_secs(1) }
    } else {
        Timing::default()
    };
    tracing::info!("zone {name}: calibration started");
    let state = state.clone();
    tokio::spawn(async move {
        let status = match calibration::run(&state, &bmc, zone, timing).await {
            Ok(reference) => {
                match reference.stall_duty {
                    Some(stall) => tracing::info!(
                        "zone {name}: calibration done, fans stop at {stall}%, suggested minimum {}%",
                        reference.suggested_min_duty.unwrap_or(100)
                    ),
                    None => tracing::info!("zone {name}: calibration done, no fan stopped"),
                }
                if let Err(e) = state.set_reference(reference) {
                    tracing::warn!("cannot save the calibration: {e:#}");
                }
                CalibrationStatus::Done { zone }
            }
            Err(e) => {
                tracing::warn!("zone {name}: calibration failed: {e:#}");
                CalibrationStatus::Failed { zone, reason: format!("{e:#}") }
            }
        };
        state.set_calibration(status);
        state.wake.notify_one();
    });
    Ok(Json(json!({ "ok": true })).into_response())
}

pub async fn cancel_calibration(State(state): State<Arc<AppState>>, Path(zone): Path<u8>) -> ApiResult {
    if state.calibration().and_then(|c| c.running_zone()).is_none_or(|(id, _)| id != zone) {
        return Err(ApiError(StatusCode::CONFLICT, "no calibration running on this zone".into()));
    }
    state.calibration_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(json!({ "ok": true })).into_response())
}

// ---- History -------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct HistoryQuery {
    /// Seconds back from now.
    range: Option<u64>,
    from: Option<u64>,
    to: Option<u64>,
    points: Option<u64>,
    /// Comma separated key prefixes (`temp:,fan:`).
    prefix: Option<String>,
}

pub async fn history(State(state): State<Arc<AppState>>, Query(q): Query<HistoryQuery>) -> ApiResult {
    let Some(history) = state.history.clone() else {
        return Err(ApiError(StatusCode::NOT_FOUND, "history is disabled".into()));
    };
    let to = q.to.unwrap_or_else(crate::now);
    let from = q.from.unwrap_or_else(|| to.saturating_sub(q.range.unwrap_or(3600)));
    let points = q.points.unwrap_or(600).clamp(10, 5000);
    let prefixes: Vec<String> =
        q.prefix.unwrap_or_default().split(',').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect();
    let data = tokio::task::spawn_blocking(move || history.query(from, to, points, &prefixes))
        .await
        .map_err(|e| anyhow::anyhow!(e))??;
    Ok(Json(data).into_response())
}

// ---- Notifications -------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TestNotificationBody {
    /// Index of a saved channel...
    index: Option<usize>,
    /// ...or a channel being edited (secrets may be placeholders).
    channel: Option<NotificationChannel>,
}

pub async fn test_notification(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TestNotificationBody>,
) -> ApiResult {
    let config = state.config();
    let channel = match (body.channel, body.index) {
        (Some(channel), index) => {
            // Placeholders are resolved against the saved channel at the same index.
            let mut incoming = config.redacted();
            let slot = match index {
                Some(i) if i < incoming.notifications.len() => {
                    incoming.notifications[i] = channel;
                    i
                }
                _ => {
                    incoming.notifications.push(channel);
                    incoming.notifications.len() - 1
                }
            };
            config.merge_from_ui(incoming).notifications.swap_remove(slot)
        }
        (None, Some(i)) => config.notifications.get(i).cloned().ok_or_else(|| bad_request("unknown channel"))?,
        (None, None) => return Err(bad_request("no channel given")),
    };
    let notification = Notification {
        title: "Test".into(),
        message: "Test notification from fanwarden".into(),
        severity: Severity::Info,
    };
    state.notifier.send(&channel, &notification).await.map_err(|e| bad_request(format!("{e:#}")))?;
    Ok(Json(json!({ "ok": true })).into_response())
}
