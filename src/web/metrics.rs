//! Prometheus exposition format, metrics prefixed with `fanwarden_`.

use std::fmt::Write;

use crate::sensors::{DiskHealth, HealthStatus};
use crate::state::Snapshot;

const PREFIX: &str = "fanwarden";

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

pub fn render(s: &Snapshot) -> String {
    let mut out = String::new();
    let mut gauge = |name: &str, help: &str, samples: Vec<(String, f64)>| {
        let _ = writeln!(out, "# HELP {PREFIX}_{name} {help}\n# TYPE {PREFIX}_{name} gauge");
        for (labels, value) in samples {
            // Readings are f32: round away the conversion noise (37.2 → 37.20000076).
            let value = (value * 100.0).round() / 100.0;
            let _ = writeln!(out, "{PREFIX}_{name}{labels} {value}");
        }
    };

    gauge(
        "cpu_temperature",
        "Temperature of CPU",
        s.sensors.cpu.map(|t| (String::new(), t as f64)).into_iter().collect(),
    );
    gauge(
        "disk_temperature",
        "Temperature of disk",
        s.sensors
            .disks
            .iter()
            .filter_map(|d| {
                let labels = format!(
                    "{{name=\"{}\",serial_number=\"{}\",type=\"{}\"}}",
                    escape(&d.name),
                    escape(&d.serial),
                    d.kind.as_str()
                );
                d.temperature.map(|t| (labels, t as f64))
            })
            .collect(),
    );
    let disk_labels =
        |name: &str, serial: &str| format!("{{name=\"{}\",serial_number=\"{}\"}}", escape(name), escape(serial));
    let health_gauge = |value: fn(&DiskHealth) -> Option<f64>| -> Vec<(String, f64)> {
        s.sensors
            .disks
            .iter()
            .filter_map(|d| d.health.as_ref().and_then(value).map(|v| (disk_labels(&d.name, &d.serial), v)))
            .collect()
    };
    gauge(
        "disk_smart_status",
        "SMART health of disk: 0 ok, 1 warning (worn or damaged media), 2 failing",
        health_gauge(|h| {
            Some(match h.status {
                HealthStatus::Ok => 0.0,
                HealthStatus::Warning => 1.0,
                HealthStatus::Failing => 2.0,
            })
        }),
    );
    gauge("disk_power_on_hours", "Power-on hours of disk", health_gauge(|h| h.power_on_hours.map(|v| v as f64)));
    gauge("disk_reallocated_sectors", "Reallocated sectors of disk", health_gauge(|h| h.reallocated.map(|v| v as f64)));
    gauge(
        "disk_pending_sectors",
        "Sectors of disk waiting to be remapped",
        health_gauge(|h| h.pending.map(|v| v as f64)),
    );
    gauge("disk_crc_errors", "Interface CRC errors of disk", health_gauge(|h| h.crc_errors.map(|v| v as f64)));
    gauge("disk_media_errors", "Media errors of NVMe disk", health_gauge(|h| h.media_errors.map(|v| v as f64)));
    gauge(
        "ipmi_temperature",
        "Temperature sensors of the BMC",
        s.sensors
            .ipmi
            .iter()
            .filter_map(|t| t.celsius.map(|c| (format!("{{name=\"{}\"}}", escape(&t.name)), c as f64)))
            .collect(),
    );
    gauge(
        "ipmi_fan_speed",
        "Ipmi fan speed recovery",
        s.fans
            .iter()
            .map(|f| (format!("{{name=\"{}\"}}", escape(&f.name)), f.rpm.map_or(-1.0, |r| r as f64)))
            .collect(),
    );
    gauge(
        "fan_speed_by_zone",
        "Fan speed for the current zone in percent",
        s.zones
            .iter()
            .filter_map(|z| {
                z.duty.map(|d| (format!("{{zone=\"{}\",id=\"{}\"}}", escape(&z.name.to_lowercase()), z.id), d as f64))
            })
            .collect(),
    );
    gauge(
        "power_watts",
        "Power draw of the server (DCMI or power supply sensors)",
        s.power_watts.map(|w| (String::new(), w as f64)).into_iter().collect(),
    );
    gauge("active_alerts", "Number of active alerts", vec![(String::new(), s.alerts.len() as f64)]);
    gauge(
        "bmc_up",
        "1 if the last BMC command succeeded",
        vec![(String::new(), if s.bmc_error.is_none() { 1.0 } else { 0.0 })],
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipmi::FanReading;

    #[test]
    fn renders_metrics() {
        let mut s = Snapshot::default();
        s.sensors.cpu = Some(41.0);
        s.power_watts = Some(112.0);
        s.fans = vec![
            FanReading { name: "FAN1".into(), rpm: Some(700), status: "ok".into() },
            FanReading { name: "FAN4".into(), rpm: None, status: "ns".into() },
        ];
        s.sensors.disks = vec![crate::sensors::DiskInfo {
            name: "sda".into(),
            serial: "S1".into(),
            model: String::new(),
            kind: crate::sensors::DiskKind::Hdd,
            state: crate::sensors::DiskState::Active,
            temperature: Some(35.0),
            error: None,
            health: Some(DiskHealth { status: HealthStatus::Warning, reallocated: Some(8), ..DiskHealth::default() }),
        }];
        let text = render(&s);
        assert!(text.contains("fanwarden_cpu_temperature 41\n"));
        assert!(text.contains("fanwarden_disk_smart_status{name=\"sda\",serial_number=\"S1\"} 1\n"));
        assert!(text.contains("fanwarden_disk_reallocated_sectors{name=\"sda\",serial_number=\"S1\"} 8\n"));
        assert!(!text.contains("fanwarden_disk_pending_sectors{"));
        assert!(text.contains("fanwarden_ipmi_fan_speed{name=\"FAN1\"} 700\n"));
        assert!(text.contains("fanwarden_ipmi_fan_speed{name=\"FAN4\"} -1\n"));
        assert!(text.contains("fanwarden_bmc_up 1\n"));
        assert!(text.contains("fanwarden_power_watts 112\n"));
    }
}
