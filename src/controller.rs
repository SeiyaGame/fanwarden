//! Control loop: read sensors, compute zone duties, apply them to the BMC.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use crate::alerts::{AlertManager, Inputs};
use crate::bmc::Bmc;
use crate::config::{Config, FanMode, Ipmi, Regulation, Zone};
use crate::curve::next_duty;
use crate::ipmi::{IpmiTool, SdrReadings};
use crate::regulator::{Gains, Inertia, Regulator};
use crate::sensors::{self, DiskInfo, Reading, SensorData, Source};
use crate::sim::Simulator;
use crate::state::{AppState, RuleStatus, Snapshot, ZoneMode, ZoneStatus};

const HWMON_ROOT: &str = "/sys/class/hwmon";
/// Check the BMC fan mode every N iterations (a BMC reset puts it back to its default).
const MODE_CHECK_EVERY: u64 = 6;

/// What a rule remembers between two cycles.
#[derive(Debug, Clone, Default)]
pub struct RuleMemory {
    /// Last duty of a curve rule (hysteresis).
    last: Option<u8>,
    /// State of a target rule.
    regulator: Regulator,
}

/// Computes the duty of a zone. `dt` is the time since the previous cycle, in seconds.
pub fn compute_zone(
    zone: &Zone,
    data: &SensorData,
    manual: Option<u8>,
    failsafe_duty: u8,
    memory: &mut Vec<RuleMemory>,
    dt: f32,
    regulation: &Regulation,
) -> ZoneStatus {
    if memory.len() != zone.rules.len() {
        *memory = vec![RuleMemory::default(); zone.rules.len()];
    }
    let mut rules = Vec::with_capacity(zone.rules.len());
    let mut failsafe = false;
    for (rule, memory) in zone.rules.iter().zip(memory.iter_mut()) {
        let source = Source::parse(&rule.source);
        let reading = source.as_ref().map_or(Reading::Missing, |s| s.read(data));
        let mut emergency = false;
        let (temperature, state, duty) = match (reading, rule.target, source.as_ref()) {
            (Reading::Temp(t), Some(target), Ok(source)) => {
                let inertia = Inertia::of(source);
                let gains = Gains {
                    emergency_margin: regulation.safety_margin(inertia),
                    ..Gains::new(inertia, target.reactivity)
                };
                let out = memory.regulator.update(
                    t,
                    target.celsius,
                    dt,
                    &gains,
                    rule.curve.duty_at(t),
                    zone.min_duty as f32,
                    zone.max_duty as f32,
                );
                emergency = out.emergency;
                (Some(t), "ok", out.duty)
            }
            (Reading::Temp(t), _, _) => {
                let duty = next_duty(&rule.curve, t, memory.last, zone.hysteresis);
                memory.last = Some(duty);
                (Some(t), "ok", duty)
            }
            (Reading::Idle, _, _) => {
                *memory = RuleMemory::default();
                (None, "idle", 0)
            }
            (Reading::Missing, _, _) => {
                *memory = RuleMemory::default();
                failsafe = true;
                (None, "missing", failsafe_duty)
            }
        };
        rules.push(RuleStatus {
            source: rule.source.clone(),
            temperature,
            state,
            duty,
            target: rule.target.map(|t| t.celsius),
            emergency,
        });
    }

    let driver = rules.iter().max_by_key(|r| r.duty).map(|r| r.source.clone());
    let (mode, duty) = if let Some(duty) = manual {
        (ZoneMode::Manual, Some(duty.min(100)))
    } else if rules.is_empty() {
        (ZoneMode::Unmanaged, None)
    } else {
        let wanted = rules.iter().map(|r| r.duty).max().unwrap_or(failsafe_duty);
        let clamped = wanted.clamp(zone.min_duty, zone.max_duty);
        if failsafe { (ZoneMode::Failsafe, Some(clamped.max(failsafe_duty))) } else { (ZoneMode::Auto, Some(clamped)) }
    };
    ZoneStatus { id: zone.id, name: zone.name.clone(), mode, duty, driver, rules }
}

#[derive(Default)]
struct DiskCache {
    disks: Vec<DiskInfo>,
    updated: Option<Instant>,
}

pub struct Controller {
    state: Arc<AppState>,
    sim: Option<Arc<Mutex<Simulator>>>,
    bmc: Bmc,
    bmc_config: Ipmi,
    disks: Arc<Mutex<DiskCache>>,
    memory: HashMap<u8, Vec<RuleMemory>>,
    last_cycle: Option<Instant>,
    /// Last duty sent per zone and when.
    applied: HashMap<u8, (u8, Instant)>,
    bmc_failures: u32,
    bmc_error: Option<String>,
    fan_mode: Option<FanMode>,
    alerts: AlertManager,
    iteration: u64,
    last_prune: Option<Instant>,
}

impl Controller {
    pub fn new(state: Arc<AppState>) -> Self {
        let config = state.config();
        let sim = state.simulate.then(|| Arc::new(Mutex::new(Simulator::default())));
        let bmc = match &sim {
            Some(sim) => Bmc::Sim(sim.clone()),
            None => Bmc::Ipmi(IpmiTool::new(config.ipmi.clone())),
        };
        Self {
            state,
            sim,
            bmc,
            bmc_config: config.ipmi,
            disks: Arc::default(),
            memory: HashMap::new(),
            last_cycle: None,
            applied: HashMap::new(),
            bmc_failures: 0,
            bmc_error: None,
            fan_mode: None,
            alerts: AlertManager::default(),
            iteration: 0,
            last_prune: None,
        }
    }

    pub async fn run(mut self, mut shutdown: watch::Receiver<bool>) {
        if self.sim.is_none() {
            tokio::spawn(disk_task(self.state.clone(), self.disks.clone(), shutdown.clone()));
        }
        loop {
            let config = self.state.config();
            self.iterate(&config).await;
            let interval = Duration::from_secs(config.general.interval_secs);
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = self.state.wake.notified() => {}
                _ = shutdown.changed() => break,
            }
        }
        self.restore_on_exit().await;
    }

    async fn restore_on_exit(&self) {
        let config = self.state.config();
        if self.state.dry_run(&config) {
            return;
        }
        let mode = config.general.exit_fan_mode;
        match self.bmc.set_fan_mode(mode).await {
            Ok(()) => tracing::info!("fan mode restored to {mode:?}"),
            Err(e) => tracing::error!("cannot restore fan mode {mode:?}: {e:#}"),
        }
    }

    async fn iterate(&mut self, config: &Config) {
        self.iteration += 1;
        let dry_run = self.state.dry_run(config);

        if self.sim.is_none() && config.ipmi != self.bmc_config {
            tracing::info!("IPMI settings changed, reconnecting");
            self.bmc = Bmc::Ipmi(IpmiTool::new(config.ipmi.clone()));
            self.bmc_config = config.ipmi.clone();
            self.fan_mode = None;
            self.applied.clear();
        }

        let sdr = match self.bmc.read_sdr().await {
            Ok(sdr) => {
                self.bmc_ok();
                sdr
            }
            Err(e) => {
                self.bmc_failed("cannot read BMC sensors", &e);
                SdrReadings::default()
            }
        };

        if !dry_run && (self.fan_mode != Some(FanMode::Full) || self.iteration.is_multiple_of(MODE_CHECK_EVERY)) {
            self.ensure_full_mode().await;
        }

        let data = self.read_sensors(config, &sdr);
        let dt = self
            .last_cycle
            .map_or(config.general.interval_secs as f32, |t| t.elapsed().as_secs_f32())
            .clamp(1.0, 300.0);
        self.last_cycle = Some(Instant::now());
        let manual = self.state.manual.lock().unwrap().clone();
        let mut zones = Vec::with_capacity(config.zones.len());
        for zone in &config.zones {
            let memory = self.memory.entry(zone.id).or_default();
            let status = compute_zone(
                zone,
                &data,
                manual.get(&zone.id).copied(),
                config.general.failsafe_duty,
                memory,
                dt,
                &config.regulation,
            );
            if let Some(duty) = status.duty {
                self.apply(config, &status, duty, dry_run).await;
            }
            zones.push(status);
        }

        let failsafe_zones = zones.iter().filter(|z| z.mode == ZoneMode::Failsafe).map(|z| z.name.clone()).collect();
        let inputs = Inputs { sensors: &data, fans: &sdr.fans, bmc_failures: self.bmc_failures, failsafe_zones };
        let conditions = self.alerts.conditions(&config.alerts, &inputs);
        let now = crate::now();
        let notifications = self.alerts.update(&config.alerts, conditions, now);
        if !notifications.is_empty() && !config.notifications.is_empty() {
            let (notifier, channels) = (self.state.notifier.clone(), config.notifications.clone());
            tokio::spawn(async move {
                for n in notifications {
                    notifier.broadcast(&channels, &n).await;
                }
            });
        }

        self.record_history(config, now, &data, &sdr, &zones).await;

        self.state.publish(Snapshot {
            ts: now,
            version: env!("CARGO_PKG_VERSION"),
            simulate: self.sim.is_some(),
            dry_run,
            fan_mode: self.fan_mode,
            bmc_error: self.bmc_error.clone(),
            sources: sensors::available_sources(&data),
            sensors: data,
            fans: sdr.fans,
            zones,
            alerts: self.alerts.active(),
        });
    }

    fn read_sensors(&self, config: &Config, sdr: &SdrReadings) -> SensorData {
        if let Some(sim) = &self.sim {
            let mut data = sim.lock().unwrap().sensors();
            data.ipmi = sdr.temperatures.clone();
            return data;
        }
        let hwmon = sensors::hwmon::read_all(std::path::Path::new(HWMON_ROOT));
        let cache = self.disks.lock().unwrap();
        let max_age = Duration::from_secs(config.disks.interval_secs * 3 + 30);
        SensorData {
            cpu: sensors::hwmon::cpu_temperature(&hwmon),
            hwmon,
            disks: cache.disks.clone(),
            disks_fresh: config.disks.enabled && cache.updated.is_some_and(|t| t.elapsed() < max_age),
            ipmi: sdr.temperatures.clone(),
        }
    }

    async fn ensure_full_mode(&mut self) {
        match self.bmc.get_fan_mode().await {
            Ok(FanMode::Full) => self.fan_mode = Some(FanMode::Full),
            Ok(mode) => {
                tracing::info!("fan mode is {mode:?}, switching to Full");
                match self.bmc.set_fan_mode(FanMode::Full).await {
                    Ok(()) => {
                        self.fan_mode = Some(FanMode::Full);
                        // The BMC dropped our duties: send them again.
                        self.applied.clear();
                    }
                    Err(e) => self.bmc_failed("cannot set fan mode", &e),
                }
            }
            Err(e) => self.bmc_failed("cannot read fan mode", &e),
        }
    }

    async fn apply(&mut self, config: &Config, zone: &ZoneStatus, duty: u8, dry_run: bool) {
        let reapply = Duration::from_secs(config.general.reapply_secs.max(10));
        let previous = self.applied.get(&zone.id).copied();
        let due = previous.is_none_or(|(d, at)| d != duty || at.elapsed() >= reapply);
        if !due {
            return;
        }
        if previous.is_none_or(|(d, _)| d != duty) {
            let from = previous.map_or("-".to_string(), |(d, _)| format!("{d}%"));
            let reason = match zone.mode {
                ZoneMode::Manual => "manual".to_string(),
                ZoneMode::Failsafe => "failsafe".to_string(),
                _ => zone
                    .rules
                    .iter()
                    .find(|r| Some(&r.source) == zone.driver.as_ref())
                    .and_then(|r| r.temperature.map(|t| format!("{} {t:.0}°C", r.source)))
                    .unwrap_or_default(),
            };
            let prefix = if dry_run { "[dry-run] " } else { "" };
            tracing::info!("{prefix}zone {}: {from} → {duty}% ({reason})", zone.name);
        }
        if dry_run {
            self.applied.insert(zone.id, (duty, Instant::now()));
            return;
        }
        match self.bmc.set_duty(zone.id, duty).await {
            Ok(()) => {
                self.bmc_ok();
                self.applied.insert(zone.id, (duty, Instant::now()));
            }
            Err(e) => self.bmc_failed(&format!("cannot set duty of zone {}", zone.name), &e),
        }
    }

    fn bmc_ok(&mut self) {
        if self.bmc_failures > 0 {
            tracing::info!("BMC reachable again");
        }
        self.bmc_failures = 0;
        self.bmc_error = None;
    }

    fn bmc_failed(&mut self, what: &str, error: &anyhow::Error) {
        self.bmc_failures += 1;
        let message = format!("{what}: {error:#}");
        // Log the first failures, then stay quiet until it recovers.
        if self.bmc_failures <= 3 {
            tracing::error!("{message}");
        }
        self.bmc_error = Some(message);
        self.fan_mode = None;
    }

    async fn record_history(
        &mut self,
        config: &Config,
        now: u64,
        data: &SensorData,
        sdr: &SdrReadings,
        zones: &[ZoneStatus],
    ) {
        let Some(history) = self.state.history.clone() else {
            return;
        };
        if !config.history.enabled {
            return;
        }
        let mut values: Vec<(String, f32)> = Vec::new();
        if let Some(cpu) = data.cpu {
            values.push(("temp:cpu".into(), cpu));
        }
        for disk in &data.disks {
            if let Some(t) = disk.temperature {
                values.push((format!("temp:disk:{}", disk.serial), t));
            }
        }
        for t in &data.ipmi {
            if let Some(c) = t.celsius {
                values.push((format!("temp:ipmi:{}", t.name), c));
            }
        }
        for fan in &sdr.fans {
            if let Some(rpm) = fan.rpm {
                values.push((format!("fan:{}", fan.name), rpm as f32));
            }
        }
        for zone in zones {
            if let Some(duty) = zone.duty {
                values.push((format!("duty:{}", zone.id), duty as f32));
            }
        }
        let prune_before = self
            .last_prune
            .is_none_or(|t| t.elapsed() > Duration::from_secs(3600))
            .then(|| now.saturating_sub(config.history.retention_days as u64 * 86_400));
        if prune_before.is_some() {
            self.last_prune = Some(Instant::now());
        }
        let result = tokio::task::spawn_blocking(move || {
            history.record(now, &values)?;
            if let Some(before) = prune_before {
                history.prune(before)?;
            }
            anyhow::Ok(())
        })
        .await;
        if let Ok(Err(e)) = result {
            tracing::warn!("cannot write history: {e:#}");
        }
    }
}

/// Disk scans are slow (one smartctl per disk): they run on their own schedule.
async fn disk_task(state: Arc<AppState>, cache: Arc<Mutex<DiskCache>>, mut shutdown: watch::Receiver<bool>) {
    let mut last_error: Option<String> = None;
    loop {
        let config = state.config();
        if config.disks.enabled {
            match sensors::disks::scan(&config.disks).await {
                Ok(disks) => {
                    if last_error.take().is_some() {
                        tracing::info!("disk scan working again");
                    }
                    let mut cache = cache.lock().unwrap();
                    cache.disks = disks;
                    cache.updated = Some(Instant::now());
                }
                Err(e) => {
                    let message = format!("{e:#}");
                    if last_error.as_ref() != Some(&message) {
                        tracing::error!("disk scan failed: {message}");
                    }
                    last_error = Some(message);
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(config.disks.interval_secs)) => {}
            _ = shutdown.changed() => return,
        }
    }
}

/// Path of the history database, next to the configuration file.
pub fn history_path(config_path: &std::path::Path) -> PathBuf {
    config_path.with_file_name("history.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Rule, Target};
    use crate::curve::Curve;
    use crate::regulator::Reactivity;
    use crate::sensors::{DiskKind, DiskState};

    fn zone(rules: Vec<Rule>) -> Zone {
        Zone { id: 1, name: "Peripheral".into(), hysteresis: 0.0, min_duty: 25, max_duty: 90, rules }
    }

    fn rule(source: &str) -> Rule {
        Rule { source: source.into(), curve: Curve::new(vec![(30.0, 30), (50.0, 100)]), target: None }
    }

    fn data(cpu: Option<f32>, hdd: Option<(DiskState, Option<f32>)>) -> SensorData {
        SensorData {
            cpu,
            disks: hdd
                .into_iter()
                .map(|(state, temperature)| DiskInfo {
                    name: "sda".into(),
                    serial: "S".into(),
                    model: String::new(),
                    kind: DiskKind::Hdd,
                    state,
                    temperature,
                    error: None,
                })
                .collect(),
            disks_fresh: true,
            ..SensorData::default()
        }
    }

    #[test]
    fn highest_rule_wins_and_limits_apply() {
        let z = zone(vec![rule("cpu"), rule("disks:hdd")]);
        let mut memory = Vec::new();
        let status = compute_zone(
            &z,
            &data(Some(40.0), Some((DiskState::Active, Some(35.0)))),
            None,
            100,
            &mut memory,
            10.0,
            &Regulation::default(),
        );
        assert_eq!(status.mode, ZoneMode::Auto);
        assert_eq!(status.duty, Some(65));
        assert_eq!(status.driver.as_deref(), Some("cpu"));

        // max_duty caps the curve.
        let status = compute_zone(
            &z,
            &data(Some(60.0), Some((DiskState::Active, Some(35.0)))),
            None,
            100,
            &mut memory,
            10.0,
            &Regulation::default(),
        );
        assert_eq!(status.duty, Some(90));
    }

    #[test]
    fn sleeping_disks_use_min_duty() {
        let z = zone(vec![rule("disks:hdd")]);
        let status = compute_zone(
            &z,
            &data(None, Some((DiskState::Standby, None))),
            None,
            100,
            &mut Vec::new(),
            10.0,
            &Regulation::default(),
        );
        assert_eq!(status.mode, ZoneMode::Auto);
        assert_eq!(status.duty, Some(25));
        assert_eq!(status.rules[0].state, "idle");
    }

    #[test]
    fn missing_source_triggers_failsafe_above_max() {
        let z = zone(vec![rule("cpu")]);
        let status = compute_zone(&z, &data(None, None), None, 100, &mut Vec::new(), 10.0, &Regulation::default());
        assert_eq!(status.mode, ZoneMode::Failsafe);
        assert_eq!(status.duty, Some(100));
    }

    #[test]
    fn target_rule_lowers_the_speed_while_below_the_target() {
        let mut target_rule = rule("disks:hdd");
        target_rule.target = Some(Target { celsius: 38.0, reactivity: Reactivity::Normal });
        let z = zone(vec![target_rule]);
        let cool = data(None, Some((DiskState::Active, Some(36.0))));
        let mut memory = Vec::new();

        // First cycle starts from the curve: 36°C → 51%.
        let first = compute_zone(&z, &cool, None, 100, &mut memory, 10.0, &Regulation::default());
        assert_eq!(first.duty, Some(51));
        assert_eq!(first.rules[0].target, Some(38.0));

        // 2°C under the target for 5 minutes: the speed goes down, never below min_duty.
        let mut duty = 51;
        for _ in 0..30 {
            duty = compute_zone(&z, &cool, None, 100, &mut memory, 10.0, &Regulation::default()).duty.unwrap();
        }
        assert!((25..=45).contains(&duty), "duty {duty}");

        // Disks asleep: the regulator restarts from the curve next time.
        compute_zone(
            &z,
            &data(None, Some((DiskState::Standby, None))),
            None,
            100,
            &mut memory,
            10.0,
            &Regulation::default(),
        );
        assert_eq!(compute_zone(&z, &cool, None, 100, &mut memory, 10.0, &Regulation::default()).duty, Some(51));
    }

    #[test]
    fn safety_margin_comes_from_the_settings() {
        let mut target_rule = rule("disks:hdd");
        target_rule.target = Some(Target { celsius: 38.0, reactivity: Reactivity::Normal });
        let z = zone(vec![target_rule]);
        // 41°C: 3°C over the target.
        let warm = data(None, Some((DiskState::Active, Some(41.0))));

        let status = compute_zone(&z, &warm, None, 100, &mut Vec::new(), 10.0, &Regulation::default());
        assert!(!status.rules[0].emergency, "default margin is 4°C");

        let tight = Regulation { safety_margin_disks: 2.0, ..Regulation::default() };
        let status = compute_zone(&z, &warm, None, 100, &mut Vec::new(), 10.0, &tight);
        assert!(status.rules[0].emergency, "3°C over a 2°C margin enforces the curve");
    }

    #[test]
    fn manual_and_unmanaged() {
        let z = zone(vec![rule("cpu")]);
        let status = compute_zone(&z, &data(None, None), Some(42), 100, &mut Vec::new(), 10.0, &Regulation::default());
        assert_eq!((status.mode, status.duty), (ZoneMode::Manual, Some(42)));

        let empty = zone(vec![]);
        let status =
            compute_zone(&empty, &data(Some(40.0), None), None, 100, &mut Vec::new(), 10.0, &Regulation::default());
        assert_eq!((status.mode, status.duty), (ZoneMode::Unmanaged, None));
    }
}
