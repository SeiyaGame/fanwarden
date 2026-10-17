//! Configuration file (TOML), loaded at startup and edited at runtime from the web UI.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::regulator::Reactivity;

/// Placeholder returned by the API instead of secrets. Sending it back keeps the stored value.
pub const SECRET_PLACEHOLDER: &str = "__unchanged__";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub general: General,
    pub ipmi: Ipmi,
    pub disks: Disks,
    pub web: Web,
    pub history: History,
    pub alerts: Alerts,
    pub notifications: Vec<NotificationChannel>,
    pub zones: Vec<Zone>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// Seconds between two control loop iterations.
    pub interval_secs: u64,
    /// Read sensors and compute speeds, but never write to the BMC.
    pub dry_run: bool,
    /// Duty applied to a zone when one of its sources cannot be read.
    pub failsafe_duty: u8,
    /// Re-send current duties every N seconds even if unchanged (the BMC may forget them).
    pub reapply_secs: u64,
    /// Fan mode restored on the BMC when the service stops.
    pub exit_fan_mode: FanMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    Standard,
    Full,
    Optimal,
    HeavyIo,
}

impl FanMode {
    pub fn raw(self) -> u8 {
        match self {
            FanMode::Standard => 0x00,
            FanMode::Full => 0x01,
            FanMode::Optimal => 0x02,
            FanMode::HeavyIo => 0x04,
        }
    }

    pub fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0x00 => Some(FanMode::Standard),
            0x01 => Some(FanMode::Full),
            0x02 => Some(FanMode::Optimal),
            0x04 => Some(FanMode::HeavyIo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpmiTarget {
    /// The BMC of this machine, through /dev/ipmi0.
    Local,
    /// A BMC reachable over the network (IPMI over LAN).
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Ipmi {
    pub target: IpmiTarget,
    pub ipmitool_path: String,
    pub timeout_secs: u64,
    pub host: String,
    pub username: String,
    pub password: String,
    /// `lanplus` (IPMI 2.0) or `lan` (IPMI 1.5).
    pub interface: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Disks {
    pub enabled: bool,
    pub smartctl_path: String,
    pub lsblk_path: String,
    pub interval_secs: u64,
    /// Regex on the device name (`sda`, `nvme0n1`...) of disks to ignore.
    pub exclude: String,
    /// Do not wake up disks in standby to read their temperature.
    pub skip_standby: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Web {
    pub listen: String,
    /// Argon2 hash of the web UI password. Empty: the UI asks to create one.
    pub password_hash: String,
    pub session_hours: u64,
    /// Expose Prometheus metrics on /metrics (no authentication).
    pub metrics_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct History {
    pub enabled: bool,
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Alerts {
    pub cpu_high: Option<f32>,
    /// HDD and SATA SSD.
    pub disk_high: Option<f32>,
    /// NVMe drives run much hotter than spinning disks.
    pub nvme_high: Option<f32>,
    pub disk_low: Option<f32>,
    /// A fan that had a reading and drops below this speed raises an alert.
    pub fan_min_rpm: Option<u32>,
    /// Consecutive failed BMC commands before raising an alert.
    pub ipmi_failures: u32,
    pub repeat_minutes: u64,
    pub notify_resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationChannel {
    Discord {
        #[serde(default = "yes")]
        enabled: bool,
        url: String,
    },
    Ntfy {
        #[serde(default = "yes")]
        enabled: bool,
        server: String,
        topic: String,
        #[serde(default)]
        token: String,
    },
    Gotify {
        #[serde(default = "yes")]
        enabled: bool,
        server: String,
        token: String,
    },
    /// Generic JSON POST: `{"title", "message", "severity"}`.
    Webhook {
        #[serde(default = "yes")]
        enabled: bool,
        url: String,
    },
}

fn yes() -> bool {
    true
}

impl NotificationChannel {
    pub fn enabled(&self) -> bool {
        match self {
            NotificationChannel::Discord { enabled, .. }
            | NotificationChannel::Ntfy { enabled, .. }
            | NotificationChannel::Gotify { enabled, .. }
            | NotificationChannel::Webhook { enabled, .. } => *enabled,
        }
    }

    fn secrets_mut(&mut self) -> Vec<&mut String> {
        match self {
            NotificationChannel::Discord { url, .. } | NotificationChannel::Webhook { url, .. } => {
                vec![url]
            }
            NotificationChannel::Ntfy { token, .. } | NotificationChannel::Gotify { token, .. } => {
                vec![token]
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Zone {
    /// IPMI zone number: 0 = CPU zone (FAN1..n), 1 = peripheral zone (FANA..).
    pub id: u8,
    pub name: String,
    /// Degrees the temperature must drop before the speed is lowered.
    pub hysteresis: f32,
    pub min_duty: u8,
    pub max_duty: u8,
    /// The zone runs at the highest duty requested by its rules.
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Temperature source, see [`crate::sensors::Source`].
    pub source: String,
    /// Speed by temperature. In target mode it is the starting point and the safety net.
    pub curve: Curve,
    /// Target mode: find the lowest speed that holds this temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub celsius: f32,
    #[serde(default)]
    pub reactivity: Reactivity,
}

impl Default for General {
    fn default() -> Self {
        Self {
            interval_secs: 10,
            dry_run: false,
            failsafe_duty: 100,
            reapply_secs: 300,
            exit_fan_mode: FanMode::Standard,
        }
    }
}

impl Default for Ipmi {
    fn default() -> Self {
        Self {
            target: IpmiTarget::Local,
            ipmitool_path: "ipmitool".into(),
            timeout_secs: 10,
            host: String::new(),
            username: "ADMIN".into(),
            password: String::new(),
            interface: "lanplus".into(),
        }
    }
}

impl Default for Disks {
    fn default() -> Self {
        Self {
            enabled: true,
            smartctl_path: "smartctl".into(),
            lsblk_path: "lsblk".into(),
            interval_secs: 60,
            exclude: "^(zram|loop|sr)".into(),
            skip_standby: true,
        }
    }
}

impl Default for Web {
    fn default() -> Self {
        Self { listen: "0.0.0.0:9495".into(), password_hash: String::new(), session_hours: 168, metrics_enabled: true }
    }
}

impl Default for History {
    fn default() -> Self {
        Self { enabled: true, retention_days: 30 }
    }
}

impl Default for Alerts {
    fn default() -> Self {
        Self {
            cpu_high: Some(85.0),
            disk_high: Some(45.0),
            nvme_high: Some(70.0),
            disk_low: Some(10.0),
            fan_min_rpm: Some(150),
            ipmi_failures: 3,
            repeat_minutes: 60,
            notify_resolved: true,
        }
    }
}

impl Default for Zone {
    fn default() -> Self {
        Self { id: 0, name: String::new(), hysteresis: 2.0, min_duty: 20, max_duty: 100, rules: Vec::new() }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            ipmi: Ipmi::default(),
            disks: Disks::default(),
            web: Web::default(),
            history: History::default(),
            alerts: Alerts::default(),
            notifications: Vec::new(),
            zones: vec![
                Zone {
                    id: 0,
                    name: "CPU".into(),
                    hysteresis: 3.0,
                    rules: vec![Rule {
                        target: None,
                        source: "cpu".into(),
                        curve: Curve::new(vec![(30.0, 30), (45.0, 45), (60.0, 60), (75.0, 85), (85.0, 100)]),
                    }],
                    ..Zone::default()
                },
                Zone {
                    id: 1,
                    name: "Peripheral".into(),
                    hysteresis: 1.0,
                    min_duty: 30,
                    rules: vec![Rule {
                        target: None,
                        source: "disks:hdd".into(),
                        curve: Curve::new(vec![
                            (30.0, 40),
                            (34.0, 60),
                            (37.0, 75),
                            (40.0, 80),
                            (45.0, 90),
                            (48.0, 100),
                        ]),
                    }],
                    ..Zone::default()
                },
            ],
        }
    }
}

impl Config {
    /// Loads the config, creating a default file if it does not exist yet.
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            let config = Config::default();
            config.save(path).with_context(|| format!("cannot create {}", path.display()))?;
            tracing::info!("created default configuration at {}", path.display());
            return Ok(config);
        }
        Self::load(path)
    }

    /// Loads and validates an existing configuration file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let config: Config =
            toml::from_str(&text).with_context(|| format!("invalid configuration {}", path.display()))?;
        config.validate().with_context(|| format!("invalid configuration {}", path.display()))?;
        Ok(config)
    }

    /// Writes atomically (temp file + rename) with owner-only permissions: the file holds secrets.
    /// The owner of an existing file is kept (e.g. PUID/PGID in Docker while running as root).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = toml::to_string_pretty(self)?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        std::fs::write(&tmp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            if let Ok(meta) = std::fs::metadata(path) {
                use std::os::unix::fs::MetadataExt;
                // Best effort: only root can give the file to another user.
                let _ = std::os::unix::fs::chown(&tmp, Some(meta.uid()), Some(meta.gid()));
            }
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.general.interval_secs < 2 {
            bail!("general.interval_secs must be at least 2");
        }
        if self.general.failsafe_duty > 100 {
            bail!("general.failsafe_duty must be between 0 and 100");
        }
        if self.ipmi.target == IpmiTarget::Remote && self.ipmi.host.trim().is_empty() {
            bail!("ipmi.host is required for a remote BMC");
        }
        if !matches!(self.ipmi.interface.as_str(), "lan" | "lanplus") {
            bail!("ipmi.interface must be 'lan' or 'lanplus'");
        }
        if self.disks.interval_secs < 5 {
            bail!("disks.interval_secs must be at least 5");
        }
        regex::Regex::new(&self.disks.exclude).context("disks.exclude is not a valid regex")?;
        if self.history.retention_days == 0 {
            bail!("history.retention_days must be at least 1");
        }

        let mut ids = std::collections::HashSet::new();
        for zone in &self.zones {
            let name = if zone.name.is_empty() { format!("zone {}", zone.id) } else { zone.name.clone() };
            if !ids.insert(zone.id) {
                bail!("{name}: zone id {} is used twice", zone.id);
            }
            if zone.min_duty > zone.max_duty || zone.max_duty > 100 {
                bail!("{name}: expected 0 <= min_duty <= max_duty <= 100");
            }
            if !(0.0..=20.0).contains(&zone.hysteresis) {
                bail!("{name}: hysteresis must be between 0 and 20");
            }
            for rule in &zone.rules {
                crate::sensors::Source::parse(&rule.source).with_context(|| format!("{name}: invalid source"))?;
                rule.curve.validate().with_context(|| format!("{name}: invalid curve for '{}'", rule.source))?;
                if let Some(target) = rule.target
                    && !(10.0..=100.0).contains(&target.celsius)
                {
                    bail!("{name}: target temperature for '{}' must be between 10 and 100°C", rule.source);
                }
            }
        }

        for channel in &self.notifications {
            let ok = match channel {
                NotificationChannel::Discord { url, .. } | NotificationChannel::Webhook { url, .. } => {
                    url.starts_with("http")
                }
                NotificationChannel::Ntfy { server, topic, .. } => server.starts_with("http") && !topic.is_empty(),
                NotificationChannel::Gotify { server, token, .. } => server.starts_with("http") && !token.is_empty(),
            };
            if !ok {
                bail!("notification channel incomplete: {channel:?}");
            }
        }
        Ok(())
    }

    /// Copy safe to send to the browser: secrets replaced by [`SECRET_PLACEHOLDER`].
    pub fn redacted(&self) -> Config {
        let mut config = self.clone();
        config.web.password_hash.clear();
        if !config.ipmi.password.is_empty() {
            config.ipmi.password = SECRET_PLACEHOLDER.into();
        }
        for channel in &mut config.notifications {
            for secret in channel.secrets_mut() {
                if !secret.is_empty() {
                    *secret = SECRET_PLACEHOLDER.into();
                }
            }
        }
        config
    }

    /// Applies a config edited in the browser: placeholders are replaced with the current secrets,
    /// and the password hash cannot be changed this way.
    pub fn merge_from_ui(&self, mut incoming: Config) -> Config {
        incoming.web.password_hash = self.web.password_hash.clone();
        if incoming.ipmi.password == SECRET_PLACEHOLDER {
            incoming.ipmi.password = self.ipmi.password.clone();
        }
        for (i, channel) in incoming.notifications.iter_mut().enumerate() {
            let previous = self
                .notifications
                .get(i)
                .cloned()
                .map(|mut c| c.secrets_mut().into_iter().map(|s| s.clone()).collect::<Vec<_>>());
            for (j, secret) in channel.secrets_mut().into_iter().enumerate() {
                if secret == SECRET_PLACEHOLDER {
                    *secret = previous.as_ref().and_then(|p| p.get(j).cloned()).unwrap_or_default();
                }
            }
        }
        incoming
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid_and_roundtrips() {
        let config = Config::default();
        config.validate().unwrap();
        let text = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let config: Config = toml::from_str("[general]\ninterval_secs = 30\n").unwrap();
        assert_eq!(config.general.interval_secs, 30);
        assert_eq!(config.zones.len(), 2);
    }

    #[test]
    fn secrets_are_redacted_and_restored() {
        let mut config = Config::default();
        config.ipmi.password = "bmc-secret".into();
        config.web.password_hash = "hash".into();
        config.notifications.push(NotificationChannel::Discord { enabled: true, url: "https://discord/x".into() });

        let redacted = config.redacted();
        assert_eq!(redacted.ipmi.password, SECRET_PLACEHOLDER);
        assert!(redacted.web.password_hash.is_empty());
        assert!(!toml::to_string(&redacted).unwrap().contains("discord/x"));

        let merged = config.merge_from_ui(redacted);
        assert_eq!(merged, config);
    }

    #[test]
    fn duplicate_zone_is_rejected() {
        let mut config = Config::default();
        config.zones[1].id = 0;
        assert!(config.validate().is_err());
    }
}
