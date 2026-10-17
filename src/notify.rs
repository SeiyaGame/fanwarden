//! Notification channels: Discord, ntfy, Gotify and generic webhooks.

use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use serde_json::json;

use crate::config::NotificationChannel;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}

#[derive(Clone)]
pub struct Notifier {
    client: reqwest::Client,
}

impl Default for Notifier {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("fan-control/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        Self { client }
    }
}

impl Notifier {
    /// Sends to every enabled channel; failures are logged, not returned.
    pub async fn broadcast(&self, channels: &[NotificationChannel], notification: &Notification) {
        for channel in channels.iter().filter(|c| c.enabled()) {
            if let Err(e) = self.send(channel, notification).await {
                tracing::warn!("notification failed: {e:#}");
            }
        }
    }

    pub async fn send(&self, channel: &NotificationChannel, n: &Notification) -> anyhow::Result<()> {
        let request = match channel {
            NotificationChannel::Discord { url, .. } => {
                let icon = match n.severity {
                    Severity::Info => "✅",
                    Severity::Warning => "⚠️",
                    Severity::Critical => "🔥",
                };
                self.client.post(url).json(&json!({ "content": format!("{icon} **{}**\n{}", n.title, n.message) }))
            }
            NotificationChannel::Ntfy { server, topic, token, .. } => {
                let priority = match n.severity {
                    Severity::Info => "default",
                    Severity::Warning => "high",
                    Severity::Critical => "urgent",
                };
                let mut request = self
                    .client
                    .post(format!("{}/{}", server.trim_end_matches('/'), topic))
                    .header("Title", &n.title)
                    .header("Priority", priority)
                    .header("Tags", "fan")
                    .body(n.message.clone());
                if !token.is_empty() {
                    request = request.bearer_auth(token);
                }
                request
            }
            NotificationChannel::Gotify { server, token, .. } => {
                let priority = match n.severity {
                    Severity::Info => 2,
                    Severity::Warning => 5,
                    Severity::Critical => 8,
                };
                self.client
                    .post(format!("{}/message", server.trim_end_matches('/')))
                    .header("X-Gotify-Key", token)
                    .json(&json!({ "title": n.title, "message": n.message, "priority": priority }))
            }
            NotificationChannel::Webhook { url, .. } => self.client.post(url).json(n),
        };
        request.send().await.context("request failed")?.error_for_status()?;
        Ok(())
    }
}
