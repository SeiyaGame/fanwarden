//! Alert detection, with repeat delay and "resolved" messages.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::calibration::SlowFan;
use crate::config::Alerts;
use crate::ipmi::FanReading;
use crate::notify::{Notification, Severity};
use crate::sensors::{DiskHealth, DiskKind, DiskState, HealthStatus, SensorData};

/// How long an alert stays up after a SMART error counter went up.
const SMART_INCREASE_SECS: u64 = 24 * 3600;

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub key: String,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveAlert {
    pub key: String,
    pub message: String,
    pub severity: Severity,
    pub since: u64,
    #[serde(skip)]
    last_sent: u64,
}

pub struct Inputs<'a> {
    pub now: u64,
    pub sensors: &'a SensorData,
    pub fans: &'a [FanReading],
    pub bmc_failures: u32,
    /// Names of zones currently at their failsafe duty.
    pub failsafe_zones: Vec<String>,
    /// A calibration stops fans on purpose.
    pub pause_fan_alerts: bool,
    /// Fans slower than at their calibration, for long enough.
    pub slow_fans: Vec<SlowFan>,
}

#[cfg(test)]
impl Default for Inputs<'_> {
    fn default() -> Self {
        static NO_SENSORS: std::sync::LazyLock<SensorData> = std::sync::LazyLock::new(SensorData::default);
        Self {
            now: 0,
            sensors: &NO_SENSORS,
            fans: &[],
            bmc_failures: 0,
            failsafe_zones: Vec::new(),
            pause_fan_alerts: false,
            slow_fans: Vec::new(),
        }
    }
}

/// Last value of a SMART error counter, and its last increase `(from, when)`.
#[derive(Debug, Clone, Copy)]
struct Counter {
    value: u64,
    increase: Option<(u64, u64)>,
}

#[derive(Default)]
pub struct AlertManager {
    active: BTreeMap<String, ActiveAlert>,
    /// Fans that reported a speed at least once: only those can "stop".
    seen_fans: HashSet<String>,
    /// SMART error counters by `serial:counter`. The first reading is the baseline: counters that
    /// were already non-zero at startup do not alert, only increases do.
    counters: HashMap<String, Counter>,
}

/// SMART counters whose increase raises an alert.
fn error_counters(health: &DiskHealth) -> [(&'static str, &'static str, Option<u64>); 4] {
    [
        ("reallocated", "reallocated sectors", health.reallocated),
        ("pending", "pending sectors", health.pending),
        ("crc_errors", "CRC errors (cable or backplane)", health.crc_errors),
        ("media_errors", "media errors", health.media_errors),
    ]
}

impl AlertManager {
    pub fn active(&self) -> Vec<ActiveAlert> {
        self.active.values().cloned().collect()
    }

    pub fn conditions(&mut self, config: &Alerts, inputs: &Inputs) -> Vec<Condition> {
        let mut out = Vec::new();
        let mut push =
            |key: String, severity: Severity, message: String| out.push(Condition { key, message, severity });

        if let (Some(limit), Some(cpu)) = (config.cpu_high, inputs.sensors.cpu)
            && cpu >= limit
        {
            push("cpu_high".into(), Severity::Critical, format!("CPU at {cpu:.0}°C (limit {limit:.0}°C)"));
        }
        for disk in &inputs.sensors.disks {
            let id = format!("{} ({})", disk.name, disk.serial);
            if let Some(t) = disk.temperature {
                let high = if disk.kind == DiskKind::Nvme { config.nvme_high } else { config.disk_high };
                if let Some(limit) = high.filter(|l| t >= *l) {
                    push(
                        format!("disk_high:{}", disk.serial),
                        Severity::Critical,
                        format!("Disk {id} at {t:.0}°C (limit {limit:.0}°C)"),
                    );
                }
                if let Some(limit) = config.disk_low.filter(|l| t <= *l) {
                    push(
                        format!("disk_low:{}", disk.serial),
                        Severity::Warning,
                        format!("Disk {id} at {t:.0}°C (low limit {limit:.0}°C)"),
                    );
                }
            }
            if disk.state == DiskState::Error {
                let reason = disk.error.clone().unwrap_or_default();
                push(
                    format!("disk_error:{}", disk.serial),
                    Severity::Warning,
                    format!("Cannot read disk {id}: {reason}"),
                );
            }
            let Some(health) = disk.health.as_ref().filter(|_| config.smart_health) else {
                continue;
            };
            if health.status == HealthStatus::Failing {
                push(
                    format!("smart_failing:{}", disk.serial),
                    Severity::Critical,
                    format!("Disk {id}: SMART reports the drive as failing, replace it"),
                );
            }
            for (counter, label, value) in error_counters(health) {
                let Some(value) = value else { continue };
                let key = format!("smart_{counter}:{}", disk.serial);
                let entry = self.counters.entry(key.clone()).or_insert(Counter { value, increase: None });
                if value > entry.value {
                    // Successive increases keep the value from before the first one.
                    let from = entry.increase.map_or(entry.value, |(from, _)| from);
                    entry.increase = Some((from, inputs.now));
                }
                entry.value = value;
                if let Some((from, _)) = entry.increase.filter(|(_, at)| inputs.now < at + SMART_INCREASE_SECS) {
                    push(key, Severity::Warning, format!("Disk {id}: {label} went from {from} to {value}"));
                }
            }
        }
        for fan in inputs.fans.iter().filter(|_| !inputs.pause_fan_alerts) {
            if fan.rpm.is_some_and(|rpm| rpm > 0) {
                self.seen_fans.insert(fan.name.clone());
            }
            if let Some(min) = config.fan_min_rpm
                && self.seen_fans.contains(&fan.name)
                && fan.rpm.is_none_or(|rpm| rpm < min)
            {
                let speed = fan.rpm.map_or("no reading".to_string(), |r| format!("{r} RPM"));
                push(
                    format!("fan:{}", fan.name),
                    Severity::Critical,
                    format!("Fan {} stopped or too slow: {speed}", fan.name),
                );
            }
        }
        if config.ipmi_failures > 0 && inputs.bmc_failures >= config.ipmi_failures {
            push(
                "bmc".into(),
                Severity::Critical,
                format!("BMC unreachable ({} failed attempts)", inputs.bmc_failures),
            );
        }
        for slow in &inputs.slow_fans {
            let percent = 100.0 * (1.0 - slow.rpm as f32 / slow.expected as f32);
            push(
                format!("fan_slow:{}", slow.name),
                Severity::Warning,
                format!(
                    "Fan {} at {} RPM, {percent:.0}% slower than at its calibration ({} RPM at {}%): dust or wear",
                    slow.name, slow.rpm, slow.expected, slow.duty
                ),
            );
        }
        for zone in &inputs.failsafe_zones {
            push(
                format!("failsafe:{zone}"),
                Severity::Warning,
                format!("Zone {zone} in failsafe: a temperature source is missing"),
            );
        }
        out
    }

    /// Updates active alerts and returns the notifications to send.
    pub fn update(&mut self, config: &Alerts, conditions: Vec<Condition>, now: u64) -> Vec<Notification> {
        let mut notifications = Vec::new();
        let current: HashSet<String> = conditions.iter().map(|c| c.key.clone()).collect();

        for condition in conditions {
            match self.active.get_mut(&condition.key) {
                Some(alert) => {
                    alert.message = condition.message.clone();
                    if config.repeat_minutes > 0 && now >= alert.last_sent + config.repeat_minutes * 60 {
                        alert.last_sent = now;
                        notifications.push(Notification {
                            title: "Still active".into(),
                            message: condition.message,
                            severity: condition.severity,
                        });
                    }
                }
                None => {
                    notifications.push(Notification {
                        title: "Alert".into(),
                        message: condition.message.clone(),
                        severity: condition.severity,
                    });
                    tracing::warn!("alert: {}", condition.message);
                    self.active.insert(
                        condition.key.clone(),
                        ActiveAlert {
                            key: condition.key,
                            message: condition.message,
                            severity: condition.severity,
                            since: now,
                            last_sent: now,
                        },
                    );
                }
            }
        }

        let resolved: Vec<String> = self.active.keys().filter(|k| !current.contains(*k)).cloned().collect();
        for key in resolved {
            if let Some(alert) = self.active.remove(&key) {
                tracing::info!("resolved: {}", alert.message);
                if config.notify_resolved {
                    notifications.push(Notification {
                        title: "Resolved".into(),
                        message: alert.message,
                        severity: Severity::Info,
                    });
                }
            }
        }
        notifications
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::DiskInfo;

    fn sensors(disk_temp: f32) -> SensorData {
        SensorData {
            cpu: Some(50.0),
            disks: vec![DiskInfo {
                name: "sda".into(),
                serial: "S1".into(),
                model: String::new(),
                kind: DiskKind::Hdd,
                state: DiskState::Active,
                temperature: Some(disk_temp),
                error: None,
                health: Some(DiskHealth { passed: Some(true), reallocated: Some(0), ..DiskHealth::default() }),
            }],
            disks_fresh: true,
            ..SensorData::default()
        }
    }

    fn fan(rpm: Option<u32>) -> FanReading {
        FanReading { name: "FAN1".into(), rpm, status: "ok".into() }
    }

    #[test]
    fn disk_alert_lifecycle() {
        let config = Alerts { repeat_minutes: 10, ..Alerts::default() };
        let mut m = AlertManager::default();
        let hot = sensors(47.0);
        let inputs = Inputs { sensors: &hot, ..Inputs::default() };

        let c = m.conditions(&config, &inputs);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, "disk_high:S1");
        assert_eq!(m.update(&config, c.clone(), 1000).len(), 1);
        // Not repeated before the delay, repeated after.
        assert!(m.update(&config, c.clone(), 1300).is_empty());
        assert_eq!(m.update(&config, c, 1600).len(), 1);
        assert_eq!(m.active().len(), 1);

        let cool = sensors(35.0);
        let inputs = Inputs { sensors: &cool, ..Inputs::default() };
        let c = m.conditions(&config, &inputs);
        let sent = m.update(&config, c, 1700);
        assert_eq!(sent[0].severity, Severity::Info);
        assert!(m.active().is_empty());
    }

    #[test]
    fn fan_alert_only_after_seen() {
        let config = Alerts::default();
        let mut m = AlertManager::default();
        let data = sensors(30.0);
        let never = [fan(None)];
        let inputs = Inputs { sensors: &data, fans: &never, ..Inputs::default() };
        assert!(m.conditions(&config, &inputs).is_empty());

        let running = [fan(Some(900))];
        let inputs = Inputs { sensors: &data, fans: &running, ..Inputs::default() };
        assert!(m.conditions(&config, &inputs).is_empty());

        let stopped = [fan(Some(0))];
        let inputs = Inputs { sensors: &data, fans: &stopped, ..Inputs::default() };
        assert_eq!(m.conditions(&config, &inputs)[0].key, "fan:FAN1");

        // A calibration stops fans on purpose.
        let inputs = Inputs { sensors: &data, fans: &stopped, pause_fan_alerts: true, ..Inputs::default() };
        assert!(m.conditions(&config, &inputs).is_empty());
    }

    fn with_health(health: DiskHealth) -> SensorData {
        let mut data = sensors(30.0);
        data.disks[0].health = Some(health);
        data
    }

    #[test]
    fn smart_counter_increase_alerts_for_a_day() {
        let config = Alerts::default();
        let mut m = AlertManager::default();
        let keys = |m: &mut AlertManager, reallocated: u64, now: u64| -> Vec<(String, String)> {
            let data = with_health(DiskHealth { reallocated: Some(reallocated), ..DiskHealth::default() });
            let inputs = Inputs { now, sensors: &data, ..Inputs::default() };
            m.conditions(&config, &inputs).into_iter().map(|c| (c.key, c.message)).collect()
        };
        // Already non-zero at startup: the baseline, no alert.
        assert!(keys(&mut m, 3, 1000).is_empty());
        assert_eq!(
            keys(&mut m, 5, 2000),
            vec![("smart_reallocated:S1".into(), "Disk sda (S1): reallocated sectors went from 3 to 5".into())]
        );
        // A further increase keeps the starting value.
        assert_eq!(keys(&mut m, 8, 3000)[0].1, "Disk sda (S1): reallocated sectors went from 3 to 8");
        assert_eq!(keys(&mut m, 8, 3000 + SMART_INCREASE_SECS - 1).len(), 1);
        assert!(keys(&mut m, 8, 3000 + SMART_INCREASE_SECS).is_empty());
    }

    #[test]
    fn failing_disk_is_critical_unless_disabled() {
        let failing =
            with_health(DiskHealth { status: HealthStatus::Failing, passed: Some(false), ..DiskHealth::default() });
        let mut m = AlertManager::default();
        let c = m.conditions(&Alerts::default(), &Inputs { sensors: &failing, ..Inputs::default() });
        assert_eq!((c[0].key.as_str(), c[0].severity), ("smart_failing:S1", Severity::Critical));

        let off = Alerts { smart_health: false, ..Alerts::default() };
        assert!(m.conditions(&off, &Inputs { sensors: &failing, ..Inputs::default() }).is_empty());
    }

    #[test]
    fn slow_fan_alert() {
        let data = sensors(30.0);
        let slow = vec![SlowFan { name: "FAN1".into(), rpm: 700, expected: 900, duty: 40 }];
        let mut m = AlertManager::default();
        let c = m.conditions(&Alerts::default(), &Inputs { sensors: &data, slow_fans: slow, ..Inputs::default() });
        assert_eq!(c[0].key, "fan_slow:FAN1");
        assert!(c[0].message.starts_with("Fan FAN1 at 700 RPM, 22% slower"), "{}", c[0].message);
    }

    #[test]
    fn bmc_failures_threshold() {
        let config = Alerts::default();
        let mut m = AlertManager::default();
        let data = sensors(30.0);
        let inputs =
            Inputs { sensors: &data, bmc_failures: 3, failsafe_zones: vec!["CPU".into()], ..Inputs::default() };
        let keys: Vec<String> = m.conditions(&config, &inputs).into_iter().map(|c| c.key).collect();
        assert_eq!(keys, vec!["bmc", "failsafe:CPU"]);
    }
}
