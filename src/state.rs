//! State shared between the control loop and the web server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use tokio::sync::{Notify, broadcast};

use crate::alerts::ActiveAlert;
use crate::config::{Config, FanMode};
use crate::events::EventLog;
use crate::history::History;
use crate::ipmi::FanReading;
use crate::notify::Notifier;
use crate::sensors::{SensorData, SourceOption};
use crate::web::auth::Sessions;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ZoneMode {
    /// Driven by its curves.
    Auto,
    /// Duty forced from the UI.
    Manual,
    /// A source is missing: running at the failsafe duty.
    Failsafe,
    /// No rule and no manual duty: the zone is left alone.
    Unmanaged,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleStatus {
    pub source: String,
    pub temperature: Option<f32>,
    /// `ok`, `idle` (disks asleep) or `missing`.
    pub state: &'static str,
    pub duty: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneStatus {
    pub id: u8,
    pub name: String,
    pub mode: ZoneMode,
    pub duty: Option<u8>,
    /// Rule requesting the highest duty.
    pub driver: Option<String>,
    pub rules: Vec<RuleStatus>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Snapshot {
    pub ts: u64,
    pub version: &'static str,
    pub simulate: bool,
    pub dry_run: bool,
    pub fan_mode: Option<FanMode>,
    pub bmc_error: Option<String>,
    pub sensors: SensorData,
    pub fans: Vec<FanReading>,
    pub zones: Vec<ZoneStatus>,
    pub sources: Vec<SourceOption>,
    pub alerts: Vec<ActiveAlert>,
}

pub struct AppState {
    pub config_path: PathBuf,
    config: RwLock<Config>,
    snapshot: RwLock<Arc<Snapshot>>,
    pub live: broadcast::Sender<Arc<Snapshot>>,
    pub manual: Mutex<HashMap<u8, u8>>,
    pub history: Option<History>,
    pub events: EventLog,
    pub sessions: Sessions,
    pub notifier: Notifier,
    /// Wakes the control loop up (config change, manual duty).
    pub wake: Notify,
    pub simulate: bool,
    /// `--dry-run` on the command line, on top of the config setting.
    pub force_dry_run: bool,
}

impl AppState {
    pub fn new(
        config_path: PathBuf,
        config: Config,
        history: Option<History>,
        events: EventLog,
        simulate: bool,
        force_dry_run: bool,
    ) -> Arc<Self> {
        let (live, _) = broadcast::channel(16);
        Arc::new(Self {
            config_path,
            config: RwLock::new(config),
            snapshot: RwLock::new(Arc::new(Snapshot { version: env!("CARGO_PKG_VERSION"), ..Snapshot::default() })),
            live,
            manual: Mutex::new(HashMap::new()),
            history,
            events,
            sessions: Sessions::default(),
            notifier: Notifier::default(),
            wake: Notify::new(),
            simulate,
            force_dry_run,
        })
    }

    pub fn config(&self) -> Config {
        // Tolerate poisoning: the panic hook reads the config too.
        self.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn dry_run(&self, config: &Config) -> bool {
        self.force_dry_run || config.general.dry_run
    }

    /// Validates, saves to disk and applies a new configuration.
    pub fn set_config(&self, config: Config) -> anyhow::Result<()> {
        config.validate()?;
        config.save(&self.config_path)?;
        *self.config.write().unwrap() = config;
        self.wake.notify_one();
        Ok(())
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.read().unwrap().clone()
    }

    pub fn publish(&self, snapshot: Snapshot) {
        let snapshot = Arc::new(snapshot);
        *self.snapshot.write().unwrap() = snapshot.clone();
        // No receiver is fine: nobody has the UI open.
        let _ = self.live.send(snapshot);
    }
}
