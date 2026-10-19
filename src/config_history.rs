//! The last versions of the configuration, to see what changed and go back to a previous one.
//! Kept in `config-history.json` next to the configuration file (it holds secrets, like it).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::config::{Config, write_private};

/// Versions kept, oldest dropped first.
pub const MAX_VERSIONS: usize = 20;

/// What produced a version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// The file as found when the service started (edited while it was stopped, or the first run).
    Startup,
    /// Saved from the web UI.
    Ui,
    /// `config.toml` edited by hand and reloaded.
    File,
    /// A previous version brought back.
    Restore { from_ts: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Version {
    pub id: u64,
    pub ts: u64,
    pub origin: Origin,
    /// Without the web password hash, which a version never changes.
    pub config: Config,
}

/// A version as sent to the browser: the TOML text, secrets replaced by labels.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VersionView {
    pub id: u64,
    pub ts: u64,
    pub origin: Origin,
    pub toml: String,
}

pub struct ConfigHistory {
    path: PathBuf,
    versions: Mutex<Vec<Version>>,
}

/// File holding the versions, next to the configuration file.
pub fn history_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name("config-history.json")
}

fn without_password(config: &Config) -> Config {
    let mut config = config.clone();
    config.web.password_hash.clear();
    config
}

impl ConfigHistory {
    pub fn load(path: PathBuf) -> Self {
        let versions = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("ignoring {}: {e}", path.display());
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Self { path, versions: Mutex::new(versions) }
    }

    /// Adds a version, unless nothing changed since the last one (a new web password alone
    /// is not a version). Returns whether one was added.
    pub fn record(&self, config: &Config, origin: Origin, now: u64) -> bool {
        let config = without_password(config);
        let mut versions = self.versions.lock().unwrap();
        if versions.last().is_some_and(|v| v.config == config) {
            return false;
        }
        let id = versions.last().map_or(1, |v| v.id + 1);
        versions.push(Version { id, ts: now, origin, config });
        let excess = versions.len().saturating_sub(MAX_VERSIONS);
        versions.drain(..excess);
        let saved = serde_json::to_string_pretty(&*versions)
            .map_err(anyhow::Error::from)
            .and_then(|text| write_private(&self.path, &text));
        if let Err(e) = saved {
            tracing::warn!("cannot save the configuration history: {e:#}");
        }
        true
    }

    pub fn get(&self, id: u64) -> Option<Version> {
        self.versions.lock().unwrap().iter().find(|v| v.id == id).cloned()
    }

    /// Newest first. Each distinct secret becomes `(secret 1)`, `(secret 2)`...: a changed
    /// token shows in the diff without being revealed.
    pub fn views(&self) -> Vec<VersionView> {
        let versions = self.versions.lock().unwrap().clone();
        let mut labels: HashMap<String, usize> = HashMap::new();
        versions
            .into_iter()
            .rev()
            .map(|mut version| {
                for secret in version.config.secrets_mut() {
                    if secret.is_empty() {
                        continue;
                    }
                    let next = labels.len() + 1;
                    let n = *labels.entry(secret.clone()).or_insert(next);
                    *secret = format!("(secret {n})");
                }
                VersionView {
                    id: version.id,
                    ts: version.ts,
                    origin: version.origin,
                    toml: toml::to_string_pretty(&version.config).unwrap_or_default(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationChannel;

    fn history() -> (tempfile::TempDir, ConfigHistory) {
        let dir = tempfile::tempdir().unwrap();
        let history = ConfigHistory::load(history_path(&dir.path().join("config.toml")));
        (dir, history)
    }

    #[test]
    fn records_changes_only() {
        let (_dir, history) = history();
        let mut config = Config::default();
        assert!(history.record(&config, Origin::Startup, 100));
        assert!(!history.record(&config, Origin::Ui, 200));
        // A new web password is not a version.
        config.web.password_hash = "hash".into();
        assert!(!history.record(&config, Origin::Ui, 300));
        config.general.interval_secs = 20;
        assert!(history.record(&config, Origin::Ui, 400));

        let views = history.views();
        assert_eq!(views.iter().map(|v| (v.id, v.ts)).collect::<Vec<_>>(), [(2, 400), (1, 100)]);
        assert!(views[0].toml.contains("interval_secs = 20"));
        assert!(views[0].toml.contains("password_hash = \"\""));
        assert_eq!(history.get(2).unwrap().config.web.password_hash, "");
    }

    #[test]
    fn keeps_the_last_versions_across_restarts() {
        let (dir, history) = history();
        let mut config = Config::default();
        for i in 0..MAX_VERSIONS as u64 + 5 {
            config.general.interval_secs = 10 + i;
            history.record(&config, Origin::Ui, i);
        }
        let reloaded = ConfigHistory::load(history_path(&dir.path().join("config.toml")));
        let ids: Vec<u64> = reloaded.views().iter().map(|v| v.id).collect();
        assert_eq!(ids.len(), MAX_VERSIONS);
        assert_eq!((ids[0], ids[MAX_VERSIONS - 1]), (25, 6));
    }

    #[test]
    fn secrets_are_labelled_not_revealed() {
        let (_dir, history) = history();
        let mut config = Config::default();
        config.ipmi.password = "bmc-pass".into();
        config.notifications.push(NotificationChannel::Ntfy {
            enabled: true,
            server: "https://ntfy.sh".into(),
            topic: "fans".into(),
            token: "tk_old".into(),
        });
        history.record(&config, Origin::Ui, 1);
        if let NotificationChannel::Ntfy { token, .. } = &mut config.notifications[0] {
            *token = "tk_new".into();
        }
        history.record(&config, Origin::Ui, 2);

        let views = history.views();
        let all: String = views.iter().map(|v| v.toml.as_str()).collect();
        assert!(!all.contains("bmc-pass") && !all.contains("tk_"));
        // Same BMC password in both versions, a different token.
        assert!(
            views[0].toml.contains("password = \"(secret 1)\"") && views[1].toml.contains("password = \"(secret 1)\"")
        );
        assert!(views[0].toml.contains("token = \"(secret 2)\""));
        assert!(views[1].toml.contains("token = \"(secret 3)\""));
        // The stored version keeps the real secrets, to restore them.
        assert_eq!(history.get(1).unwrap().config.ipmi.password, "bmc-pass");
    }
}
