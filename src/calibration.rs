//! Fan calibration: a zone is swept from 100% down to 0% while the speed of each fan is
//! recorded. It finds the duty under which fans stop (to suggest a minimum speed) and keeps
//! the speeds as a reference, to spot a fan that turns slower than it used to (dust, wear).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::bmc::Bmc;
use crate::config::Alerts;
use crate::ipmi::FanReading;
use crate::sensors::{DiskKind, SensorData};
use crate::state::AppState;

/// Duty decrease between two steps.
pub const STEP: u8 = 5;
/// Margin added above the lowest duty at which every fan still turns: a stopped fan needs a
/// bit more to start again.
const MIN_DUTY_MARGIN: u8 = 5;
/// A fan turning this much faster than at the previous, higher duty: the BMC took over.
const TAKEOVER_RATIO: f32 = 1.25;
const TAKEOVER_RPM: f32 = 100.0;
/// Below these speeds the reference is too coarse to compare against.
const MIN_REFERENCE_RPM: f32 = 300.0;
const MIN_SLOWDOWN_RPM: f32 = 150.0;
/// Limits used during a calibration when the matching alert is disabled.
const FALLBACK_CPU_LIMIT: f32 = 85.0;
const FALLBACK_DISK_LIMIT: f32 = 45.0;
const FALLBACK_NVME_LIMIT: f32 = 70.0;

/// Speeds of one fan during a calibration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanCurve {
    pub name: String,
    /// `(duty, rpm)` from 100% down; 0 RPM when the fan stopped.
    pub points: Vec<(u8, u32)>,
    /// Highest duty at which this fan stopped.
    pub stall_duty: Option<u8>,
}

/// Result of a calibration, kept as the reference of the zone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reference {
    pub zone: u8,
    pub ts: u64,
    pub fans: Vec<FanCurve>,
    /// Highest duty at which a fan of the zone stopped.
    pub stall_duty: Option<u8>,
    /// A step and a margin above the lowest duty that is safe: where the fans still turn, or
    /// where the BMC still leaves them alone.
    pub suggested_min_duty: Option<u8>,
    /// Duty at which the BMC raised the fans on its own (fan below its RPM threshold):
    /// the sweep stopped there.
    pub bmc_takeover: Option<u8>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum CalibrationStatus {
    Running {
        zone: u8,
        duty: u8,
        step: usize,
        steps: usize,
    },
    /// The reference of the zone holds the result.
    Done {
        zone: u8,
    },
    Failed {
        zone: u8,
        reason: String,
    },
}

impl CalibrationStatus {
    /// Zone being swept, whose duty the control loop must leave alone.
    pub fn running_zone(&self) -> Option<(u8, u8)> {
        match self {
            CalibrationStatus::Running { zone, duty, .. } => Some((*zone, *duty)),
            _ => None,
        }
    }
}

/// Supermicro naming: FAN1..n belong to zone 0 (CPU), FANA..n to zone 1 (peripheral).
pub fn in_zone(zone: u8, fan: &str) -> bool {
    let Some(rest) = fan.to_ascii_uppercase().strip_prefix("FAN").map(str::to_string) else {
        return false;
    };
    match zone {
        0 => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
        1 => rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphabetic()),
        _ => false,
    }
}

/// Duties of a sweep: 100, 95, ... 0.
pub fn duties() -> Vec<u8> {
    (0..=100 / STEP).rev().map(|i| i * STEP).collect()
}

/// Readings collected step by step.
#[derive(Debug)]
pub struct Sweep {
    zone: u8,
    fans: Vec<FanCurve>,
    takeover: Option<u8>,
}

impl Sweep {
    pub fn new(zone: u8) -> Self {
        Self { zone, fans: Vec::new(), takeover: None }
    }

    /// Records one step. Returns false when the BMC took over and the sweep must stop.
    pub fn record(&mut self, duty: u8, readings: &[FanReading]) -> bool {
        let readings: Vec<&FanReading> = readings.iter().filter(|f| in_zone(self.zone, &f.name)).collect();
        if self.fans.is_empty() {
            // First step, at 100%: fans without a speed are not connected.
            self.fans = readings
                .iter()
                .filter(|f| f.rpm.is_some_and(|r| r > 0))
                .map(|f| FanCurve { name: f.name.clone(), points: Vec::new(), stall_duty: None })
                .collect();
        }
        let rpms: Vec<u32> = self
            .fans
            .iter()
            .map(|fan| readings.iter().find(|r| r.name == fan.name).and_then(|r| r.rpm).unwrap_or(0))
            .collect();
        let takeover = self.fans.iter().zip(&rpms).any(|(fan, &rpm)| {
            fan.points.last().is_some_and(|&(_, previous)| rpm as f32 > previous as f32 * TAKEOVER_RATIO + TAKEOVER_RPM)
        });
        if takeover {
            self.takeover = Some(duty);
            return false;
        }
        for (fan, rpm) in self.fans.iter_mut().zip(rpms) {
            fan.points.push((duty, rpm));
            if rpm == 0 && fan.stall_duty.is_none() {
                fan.stall_duty = Some(duty);
            }
        }
        true
    }

    pub fn has_fans(&self) -> bool {
        !self.fans.is_empty()
    }

    pub fn finish(self, ts: u64) -> Reference {
        let stall_duty = self.fans.iter().filter_map(|f| f.stall_duty).max();
        // Without a stall, the BMC stopping the sweep sets the floor: the last step it accepted
        // sits right at a fan threshold, so stay a step and a margin above it.
        let lowest_accepted =
            self.takeover.and_then(|_| self.fans.iter().filter_map(|f| f.points.last()).map(|p| p.0).max());
        Reference {
            zone: self.zone,
            ts,
            suggested_min_duty: stall_duty.or(lowest_accepted).map(|d| (d + STEP + MIN_DUTY_MARGIN).min(100)),
            stall_duty,
            bmc_takeover: self.takeover,
            fans: self.fans,
        }
    }
}

/// Speed a fan had at this duty during its calibration, if it was turning.
pub fn expected_rpm(curve: &FanCurve, duty: u8) -> Option<f32> {
    let above = curve.points.iter().filter(|(d, _)| *d >= duty).min_by_key(|(d, _)| *d)?;
    let below = curve.points.iter().filter(|(d, _)| *d <= duty).max_by_key(|(d, _)| *d)?;
    if above.1 == 0 || below.1 == 0 {
        return None;
    }
    if above.0 == below.0 {
        return Some(above.1 as f32);
    }
    let ratio = (duty - below.0) as f32 / (above.0 - below.0) as f32;
    Some(below.1 as f32 + ratio * (above.1 as f32 - below.1 as f32))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlowFan {
    pub name: String,
    pub rpm: u32,
    pub expected: u32,
    pub duty: u8,
}

/// Fans turning `percent`% or more under their reference at the current duty.
pub fn slow_fans(reference: &Reference, duty: u8, fans: &[FanReading], percent: f32) -> Vec<SlowFan> {
    reference
        .fans
        .iter()
        .filter_map(|curve| {
            let expected = expected_rpm(curve, duty).filter(|e| *e >= MIN_REFERENCE_RPM)?;
            let rpm = fans.iter().find(|f| f.name == curve.name)?.rpm?;
            let slow = (rpm as f32) < expected * (1.0 - percent / 100.0) && expected - rpm as f32 >= MIN_SLOWDOWN_RPM;
            slow.then(|| SlowFan { name: curve.name.clone(), rpm, expected: expected.round() as u32, duty })
        })
        .collect()
}

/// A temperature over its alert limit (or a fallback when the alert is off): stop the sweep.
pub fn overheating(alerts: &Alerts, sensors: &SensorData) -> Option<String> {
    let cpu_limit = alerts.cpu_high.unwrap_or(FALLBACK_CPU_LIMIT);
    if let Some(cpu) = sensors.cpu.filter(|t| *t >= cpu_limit) {
        return Some(format!("CPU reached {cpu:.0}°C (limit {cpu_limit:.0}°C)"));
    }
    sensors.disks.iter().find_map(|disk| {
        let limit = match disk.kind {
            DiskKind::Nvme => alerts.nvme_high.unwrap_or(FALLBACK_NVME_LIMIT),
            _ => alerts.disk_high.unwrap_or(FALLBACK_DISK_LIMIT),
        };
        let t = disk.temperature.filter(|t| *t >= limit)?;
        Some(format!("disk {} reached {t:.0}°C (limit {limit:.0}°C)", disk.name))
    })
}

/// Timings of a sweep; shorter in tests.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Wait at 100% before the first reading.
    pub spin_up: Duration,
    /// Wait after each duty change.
    pub settle: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self { spin_up: Duration::from_secs(15), settle: Duration::from_secs(10) }
    }
}

/// Sweeps a zone and returns its reference. The fans are left at 100% on failure; the control
/// loop takes the zone back as soon as the status is no longer `Running`.
pub async fn run(state: &Arc<AppState>, bmc: &Bmc, zone: u8, timing: Timing) -> anyhow::Result<Reference> {
    let duties = duties();
    let mut sweep = Sweep::new(zone);
    for (step, &duty) in duties.iter().enumerate() {
        state.set_calibration(CalibrationStatus::Running { zone, duty, step: step + 1, steps: duties.len() });
        if let Err(e) = bmc.set_duty(zone, duty).await {
            return fail(bmc, zone, e.context(format!("cannot set the duty to {duty}%"))).await;
        }
        // The control loop refreshes the sensors the safety check reads.
        state.wake.notify_one();
        wait(state, if step == 0 { timing.spin_up } else { timing.settle }).await?;
        if let Some(reason) = overheating(&state.config().alerts, &state.snapshot().sensors) {
            return fail(bmc, zone, anyhow::anyhow!("stopped: {reason}")).await;
        }
        let sdr = match bmc.read_sdr().await {
            Ok(sdr) => sdr,
            Err(e) => return fail(bmc, zone, e.context("cannot read the fan speeds")).await,
        };
        let keep_going = sweep.record(duty, &sdr.fans);
        if !sweep.has_fans() {
            return fail(bmc, zone, anyhow::anyhow!("no fan of this zone reports a speed")).await;
        }
        if !keep_going {
            tracing::warn!("calibration of zone {zone}: the BMC raised the fans at {duty}% (fan thresholds)");
            break;
        }
    }
    // The sweep ends with the fans stopped: full speed restarts them before the zone is handed back.
    if let Err(e) = bmc.set_duty(zone, 100).await {
        tracing::warn!("calibration of zone {zone}: cannot put the fans back to 100%: {e:#}");
    }
    Ok(sweep.finish(crate::now()))
}

/// Waits, returning early with an error when the calibration is cancelled.
async fn wait(state: &AppState, total: Duration) -> anyhow::Result<()> {
    let slice = Duration::from_millis(200).min(total.max(Duration::from_millis(1)));
    let mut waited = Duration::ZERO;
    while waited < total {
        if state.calibration_cancel.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        tokio::time::sleep(slice).await;
        waited += slice;
    }
    if state.calibration_cancel.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }
    Ok(())
}

async fn fail(bmc: &Bmc, zone: u8, error: anyhow::Error) -> anyhow::Result<Reference> {
    if let Err(e) = bmc.set_duty(zone, 100).await {
        tracing::error!("calibration of zone {zone}: cannot put the fans back to 100%: {e:#}");
    }
    Err(error)
}

/// File holding the references, next to the configuration file.
pub fn references_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name("calibration.json")
}

pub fn load_references(path: &Path) -> BTreeMap<u8, Reference> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    match serde_json::from_str::<Vec<Reference>>(&text) {
        Ok(list) => list.into_iter().map(|r| (r.zone, r)).collect(),
        Err(e) => {
            tracing::warn!("ignoring {}: {e}", path.display());
            BTreeMap::new()
        }
    }
}

pub fn save_references(path: &Path, references: &BTreeMap<u8, Reference>) -> anyhow::Result<()> {
    let list: Vec<&Reference> = references.values().collect();
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, serde_json::to_string_pretty(&list)?)
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::{DiskInfo, DiskState};

    fn fan(name: &str, rpm: Option<u32>) -> FanReading {
        FanReading { name: name.into(), rpm, status: "ok".into() }
    }

    #[test]
    fn fans_by_zone() {
        assert!(in_zone(0, "FAN1") && in_zone(0, "FAN12"));
        assert!(in_zone(1, "FANA") && in_zone(1, "fanb"));
        assert!(!in_zone(0, "FANA") && !in_zone(1, "FAN1") && !in_zone(0, "FAN") && !in_zone(2, "FAN1"));
    }

    #[test]
    fn sweep_finds_where_fans_stop() {
        let mut sweep = Sweep::new(0);
        for duty in duties() {
            // FAN1 stops under 20%, FAN2 under 10%, FAN4 is not connected, FANA is another zone.
            let rpm = |stall: u8| Some(if duty < stall { 0 } else { 300 + duty as u32 * 15 });
            assert!(sweep.record(
                duty,
                &[fan("FAN1", rpm(20)), fan("FAN2", rpm(10)), fan("FAN4", None), fan("FANA", Some(900))]
            ));
        }
        let reference = sweep.finish(1000);
        let names: Vec<&str> = reference.fans.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["FAN1", "FAN2"]);
        assert_eq!(reference.fans[0].stall_duty, Some(15));
        assert_eq!(reference.fans[1].stall_duty, Some(5));
        assert_eq!(reference.fans[0].points.len(), 21);
        assert_eq!((reference.stall_duty, reference.suggested_min_duty), (Some(15), Some(25)));
        assert_eq!(reference.bmc_takeover, None);
    }

    #[test]
    fn fans_that_never_stop_suggest_nothing() {
        let mut sweep = Sweep::new(1);
        for duty in duties() {
            sweep.record(duty, &[fan("FANA", Some(300 + duty as u32 * 10))]);
        }
        let reference = sweep.finish(0);
        assert_eq!((reference.stall_duty, reference.suggested_min_duty), (None, None));
    }

    #[test]
    fn sweep_stops_when_the_bmc_takes_over() {
        let mut sweep = Sweep::new(0);
        assert!(sweep.record(100, &[fan("FAN1", Some(1800))]));
        assert!(sweep.record(95, &[fan("FAN1", Some(1700))]));
        assert!(sweep.record(90, &[fan("FAN1", Some(1650))]));
        // Fan below the BMC threshold: everything back to full speed.
        assert!(!sweep.record(85, &[fan("FAN1", Some(2500))]));
        let reference = sweep.finish(0);
        assert_eq!(reference.bmc_takeover, Some(85));
        assert_eq!(reference.fans[0].points, vec![(100, 1800), (95, 1700), (90, 1650)]);
        // No fan stopped: the floor comes from the last step the BMC accepted (90%).
        assert_eq!((reference.stall_duty, reference.suggested_min_duty), (None, Some(100)));
    }

    #[test]
    fn takeover_near_the_bottom_suggests_a_margin_above_it() {
        // Fans still turning at 10% (300 RPM, a BMC threshold), the BMC takes over at 5%.
        let mut sweep = Sweep::new(1);
        for duty in duties() {
            let rpm = if duty == 5 { 3000 } else { 300 + duty as u32 * 25 };
            if !sweep.record(duty, &[fan("FANA", Some(rpm)), fan("FANB", Some(rpm + 100))]) {
                break;
            }
        }
        let reference = sweep.finish(0);
        assert_eq!(reference.bmc_takeover, Some(5));
        assert_eq!((reference.stall_duty, reference.suggested_min_duty), (None, Some(20)));
    }

    #[test]
    fn fans_that_never_stop_nor_trigger_the_bmc_suggest_nothing() {
        let mut sweep = Sweep::new(0);
        for duty in duties() {
            sweep.record(duty, &[fan("FAN1", Some(400 + duty as u32 * 18))]);
        }
        assert_eq!(sweep.finish(0).suggested_min_duty, None);
    }

    fn curve() -> FanCurve {
        FanCurve { name: "FAN1".into(), points: vec![(50, 1050), (45, 975), (40, 900), (15, 0)], stall_duty: Some(15) }
    }

    #[test]
    fn expected_speed_is_interpolated() {
        assert_eq!(expected_rpm(&curve(), 45), Some(975.0));
        assert_eq!(expected_rpm(&curve(), 42), Some(930.0));
        // Between a turning point and a stopped one: no reliable value.
        assert_eq!(expected_rpm(&curve(), 30), None);
        assert_eq!(expected_rpm(&curve(), 60), None);
    }

    #[test]
    fn slow_fan_is_reported() {
        let reference = Reference {
            zone: 0,
            ts: 0,
            fans: vec![curve()],
            stall_duty: Some(15),
            suggested_min_duty: Some(20),
            bmc_takeover: None,
        };
        assert!(slow_fans(&reference, 40, &[fan("FAN1", Some(850))], 15.0).is_empty());
        let slow = slow_fans(&reference, 40, &[fan("FAN1", Some(700))], 15.0);
        assert_eq!(slow, vec![SlowFan { name: "FAN1".into(), rpm: 700, expected: 900, duty: 40 }]);
        // No reading or no reference at this duty: nothing to compare.
        assert!(slow_fans(&reference, 40, &[fan("FAN1", None)], 15.0).is_empty());
        assert!(slow_fans(&reference, 20, &[fan("FAN1", Some(10))], 15.0).is_empty());
    }

    #[test]
    fn overheating_uses_the_alert_limits() {
        let disk = |t: f32| DiskInfo {
            name: "sda".into(),
            serial: "S".into(),
            model: String::new(),
            kind: DiskKind::Hdd,
            state: DiskState::Active,
            temperature: Some(t),
            error: None,
            health: None,
        };
        let alerts = Alerts { disk_high: Some(43.0), ..Alerts::default() };
        let cool = SensorData { cpu: Some(60.0), disks: vec![disk(40.0)], ..SensorData::default() };
        assert_eq!(overheating(&alerts, &cool), None);
        let hot = SensorData { disks: vec![disk(43.0)], ..cool.clone() };
        assert_eq!(overheating(&alerts, &hot).unwrap(), "disk sda reached 43°C (limit 43°C)");
        // Alert disabled: the fallback limit still protects the disks.
        let off = Alerts { disk_high: None, ..alerts };
        assert_eq!(overheating(&off, &hot), None);
        assert!(overheating(&off, &SensorData { disks: vec![disk(46.0)], ..cool }).is_some());
    }

    #[test]
    fn references_roundtrip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = references_path(&dir.path().join("config.toml"));
        assert!(load_references(&path).is_empty());
        let mut sweep = Sweep::new(1);
        sweep.record(100, &[fan("FANA", Some(1600))]);
        let reference = sweep.finish(42);
        save_references(&path, &BTreeMap::from([(1, reference.clone())])).unwrap();
        assert_eq!(load_references(&path).get(&1), Some(&reference));
    }

    fn simulated() -> (tempfile::TempDir, Arc<AppState>, Bmc, Arc<std::sync::Mutex<crate::sim::Simulator>>) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(
            dir.path().join("config.toml"),
            crate::config::Config::default(),
            None,
            crate::events::EventLog::default(),
            true,
            false,
        );
        let sim = Arc::new(std::sync::Mutex::new(crate::sim::Simulator::default()));
        sim.lock().unwrap().mode = crate::config::FanMode::Full;
        (dir, state, Bmc::Sim(sim.clone()), sim)
    }

    const NO_WAIT: Timing = Timing { spin_up: Duration::ZERO, settle: Duration::ZERO };

    #[tokio::test]
    async fn sweep_of_the_simulated_zone() {
        let (_dir, state, bmc, sim) = simulated();
        // Simulated peripheral fans stop under 20%.
        let reference = run(&state, &bmc, 1, NO_WAIT).await.unwrap();
        let names: Vec<&str> = reference.fans.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["FANA", "FANB"]);
        assert_eq!((reference.stall_duty, reference.suggested_min_duty), (Some(15), Some(25)));
        assert_eq!(state.calibration(), Some(CalibrationStatus::Running { zone: 1, duty: 0, step: 21, steps: 21 }));
        // The fans are restarted at full speed before the zone is handed back.
        let fana = sim.lock().unwrap().sdr().fans.into_iter().find(|f| f.name == "FANA").unwrap();
        assert_eq!(fana.rpm, Some(1900));
    }

    #[tokio::test]
    async fn cancelled_sweep_puts_the_fans_back_to_full_speed() {
        let (_dir, state, bmc, sim) = simulated();
        state.calibration_cancel.store(true, Ordering::Relaxed);
        let error = run(&state, &bmc, 0, NO_WAIT).await.unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
        let fan1 = sim.lock().unwrap().sdr().fans.into_iter().find(|f| f.name == "FAN1").unwrap();
        assert_eq!(fan1.rpm, Some(1800));
    }
}
