//! Alert detection, with repeat delay and "resolved" messages.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::config::Alerts;
use crate::ipmi::FanReading;
use crate::notify::{Notification, Severity};
use crate::sensors::{DiskKind, DiskState, SensorData};

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
    pub sensors: &'a SensorData,
    pub fans: &'a [FanReading],
    pub bmc_failures: u32,
    /// Names of zones currently at their failsafe duty.
    pub failsafe_zones: Vec<String>,
}

#[derive(Default)]
pub struct AlertManager {
    active: BTreeMap<String, ActiveAlert>,
    /// Fans that reported a speed at least once: only those can "stop".
    seen_fans: HashSet<String>,
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
        }
        for fan in inputs.fans {
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
        let inputs = Inputs { sensors: &hot, fans: &[], bmc_failures: 0, failsafe_zones: vec![] };

        let c = m.conditions(&config, &inputs);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, "disk_high:S1");
        assert_eq!(m.update(&config, c.clone(), 1000).len(), 1);
        // Not repeated before the delay, repeated after.
        assert!(m.update(&config, c.clone(), 1300).is_empty());
        assert_eq!(m.update(&config, c, 1600).len(), 1);
        assert_eq!(m.active().len(), 1);

        let cool = sensors(35.0);
        let inputs = Inputs { sensors: &cool, fans: &[], bmc_failures: 0, failsafe_zones: vec![] };
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
        let inputs = Inputs { sensors: &data, fans: &never, bmc_failures: 0, failsafe_zones: vec![] };
        assert!(m.conditions(&config, &inputs).is_empty());

        let running = [fan(Some(900))];
        let inputs = Inputs { sensors: &data, fans: &running, bmc_failures: 0, failsafe_zones: vec![] };
        assert!(m.conditions(&config, &inputs).is_empty());

        let stopped = [fan(Some(0))];
        let inputs = Inputs { sensors: &data, fans: &stopped, bmc_failures: 0, failsafe_zones: vec![] };
        assert_eq!(m.conditions(&config, &inputs)[0].key, "fan:FAN1");
    }

    #[test]
    fn bmc_failures_threshold() {
        let config = Alerts::default();
        let mut m = AlertManager::default();
        let data = sensors(30.0);
        let inputs = Inputs { sensors: &data, fans: &[], bmc_failures: 3, failsafe_zones: vec!["CPU".into()] };
        let keys: Vec<String> = m.conditions(&config, &inputs).into_iter().map(|c| c.key).collect();
        assert_eq!(keys, vec!["bmc", "failsafe:CPU"]);
    }
}
