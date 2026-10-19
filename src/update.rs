//! Looks for a new release on GitHub, to show it in the web UI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::state::AppState;

const LATEST_RELEASE: &str = "https://api.github.com/repos/SeiyaGame/fanwarden/releases/latest";
const CHECK_EVERY: Duration = Duration::from_secs(12 * 3600);
/// After a failure (offline server), try again sooner.
const RETRY_AFTER: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// Markdown changelog of the release.
    pub notes: String,
    pub published: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current: &'static str,
    /// Set by the CI on the dev pre-release, e.g. `dev d7693c8`.
    pub build: Option<&'static str>,
    pub enabled: bool,
    pub latest: Option<Release>,
    /// The latest release is newer than this binary.
    pub available: bool,
    pub checked: Option<u64>,
    pub error: Option<String>,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            current: env!("CARGO_PKG_VERSION"),
            build: option_env!("FANWARDEN_BUILD").filter(|b| !b.is_empty()),
            enabled: false,
            latest: None,
            available: false,
            checked: None,
            error: None,
        }
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
}

/// `v0.4.0` or `0.4.0-rc.1` → (0, 4, 0). The pre-release suffix is ignored.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

async fn fetch(client: &reqwest::Client) -> anyhow::Result<Release> {
    let release: GithubRelease = client
        .get(LATEST_RELEASE)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("unexpected reply from GitHub")?;
    Ok(Release {
        version: release.tag_name.trim_start_matches('v').to_string(),
        url: release.html_url,
        notes: release.body.unwrap_or_default(),
        published: release.published_at.unwrap_or_default(),
    })
}

/// Checks at startup then twice a day, while `general.check_updates` is on.
pub async fn task(state: Arc<AppState>, mut shutdown: watch::Receiver<bool>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("fanwarden/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP client");
    let mut announced: Option<String> = None;
    loop {
        let enabled = state.config().general.check_updates;
        let mut wait = CHECK_EVERY;
        if enabled {
            let outcome = fetch(&client).await;
            let mut status = state.update_status();
            status.checked = Some(crate::now());
            match outcome {
                Ok(release) => {
                    status.available = is_newer(&release.version, status.current);
                    if status.available && announced.as_ref() != Some(&release.version) {
                        tracing::info!("fanwarden {} is available: {}", release.version, release.url);
                        announced = Some(release.version.clone());
                    }
                    status.latest = Some(release);
                    status.error = None;
                }
                Err(e) => {
                    status.error = Some(format!("{e:#}"));
                    wait = RETRY_AFTER;
                }
            }
            state.set_update_status(UpdateStatus { enabled, ..status });
        } else {
            state.set_update_status(UpdateStatus::default());
        }
        // Wakes up every minute to check at once when the setting gets turned on.
        let since = Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                _ = shutdown.changed() => return,
            }
            if since.elapsed() >= wait || (!enabled && state.config().general.check_updates) {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("0.4.0-rc.1", "0.4.0"));
        assert!(!is_newer("nightly", "0.3.0"));
    }

    #[test]
    fn reads_the_github_reply() {
        let release: GithubRelease = serde_json::from_str(
            r####"{"tag_name":"v0.4.0","html_url":"https://github.com/SeiyaGame/fanwarden/releases/tag/v0.4.0",
                "body":"### Features\n\n- BMC log","published_at":"2026-10-20T08:00:00Z","assets":[]}"####,
        )
        .unwrap();
        assert_eq!(release.tag_name, "v0.4.0");
        assert_eq!(release.body.as_deref(), Some("### Features\n\n- BMC log"));
    }
}
