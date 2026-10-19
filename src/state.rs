//! State shared between the control loop and the web server.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{Notify, broadcast};

use crate::alerts::ActiveAlert;
use crate::bmc::Bmc;
use crate::calibration::{self, CalibrationStatus, Reference};
use crate::config::{Config, FanMode};
use crate::config_history::{self, ConfigHistory, Origin};
use crate::events::EventLog;
use crate::history::History;
use crate::ipmi::FanReading;
use crate::notify::Notifier;
use crate::sel::BmcLog;
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
    /// Swept by a calibration.
    Calibrating,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleStatus {
    pub source: String,
    pub temperature: Option<f32>,
    /// `ok`, `idle` (disks asleep) or `missing`.
    pub state: &'static str,
    pub duty: u8,
    /// Target temperature when the rule is in target mode.
    pub target: Option<f32>,
    /// Target mode only: far above the target, the curve is enforced.
    pub emergency: bool,
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
    /// Calibration running, or the outcome of the last one.
    pub calibration: Option<CalibrationStatus>,
    /// Calibration references by zone.
    pub references: BTreeMap<u8, Reference>,
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
    /// BMC used by the control loop, shared with calibrations.
    pub bmc: RwLock<Option<Bmc>>,
    calibration: Mutex<Option<CalibrationStatus>>,
    calibration_ended: Mutex<Option<Instant>>,
    pub calibration_cancel: AtomicBool,
    references: RwLock<BTreeMap<u8, Reference>>,
    pub config_history: ConfigHistory,
    bmc_log: RwLock<BmcLog>,
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
        let references = calibration::load_references(&calibration::references_path(&config_path));
        // The file may have been edited while the service was stopped.
        let config_history = ConfigHistory::load(config_history::history_path(&config_path));
        config_history.record(&config, Origin::Startup, crate::now());
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
            bmc: RwLock::new(None),
            calibration: Mutex::new(None),
            calibration_ended: Mutex::new(None),
            calibration_cancel: AtomicBool::new(false),
            references: RwLock::new(references),
            config_history,
            bmc_log: RwLock::default(),
        })
    }

    pub fn calibration(&self) -> Option<CalibrationStatus> {
        self.calibration.lock().unwrap().clone()
    }

    pub fn set_calibration(&self, status: CalibrationStatus) {
        if status.running_zone().is_none() {
            *self.calibration_ended.lock().unwrap() = Some(Instant::now());
        }
        *self.calibration.lock().unwrap() = Some(status);
    }

    /// A calibration is running, or ended less than `within` ago.
    pub fn calibrated_within(&self, within: Duration) -> bool {
        self.calibration().is_some_and(|c| c.running_zone().is_some())
            || self.calibration_ended.lock().unwrap().is_some_and(|t| t.elapsed() < within)
    }

    pub fn bmc_log(&self) -> BmcLog {
        self.bmc_log.read().unwrap().clone()
    }

    pub fn set_bmc_log(&self, log: BmcLog) {
        *self.bmc_log.write().unwrap() = log;
    }

    pub fn update_bmc_log(&self, change: impl FnOnce(&mut BmcLog)) {
        change(&mut self.bmc_log.write().unwrap());
    }

    /// Marks a calibration as started, unless one is already running.
    pub fn try_start_calibration(&self, zone: u8) -> bool {
        let mut current = self.calibration.lock().unwrap();
        if current.as_ref().is_some_and(|c| c.running_zone().is_some()) {
            return false;
        }
        self.calibration_cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        *current = Some(CalibrationStatus::Running { zone, duty: 100, step: 0, steps: calibration::duties().len() });
        true
    }

    pub fn references(&self) -> BTreeMap<u8, Reference> {
        self.references.read().unwrap().clone()
    }

    /// Stores a new reference and writes the file.
    pub fn set_reference(&self, reference: Reference) -> anyhow::Result<()> {
        let mut references = self.references.write().unwrap();
        references.insert(reference.zone, reference);
        calibration::save_references(&calibration::references_path(&self.config_path), &references)
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

    /// Re-reads the configuration file (edited by hand) and applies it.
    /// On error the running configuration is kept.
    pub fn reload_config(&self) -> anyhow::Result<Config> {
        let config = Config::load(&self.config_path)?;
        *self.config.write().unwrap() = config.clone();
        self.wake.notify_one();
        Ok(config)
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
