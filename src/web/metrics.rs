//! Prometheus exposition format, metrics prefixed with `fanwarden_`.

use std::fmt::Write;

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
        s.fans = vec![
            FanReading { name: "FAN1".into(), rpm: Some(700), status: "ok".into() },
            FanReading { name: "FAN4".into(), rpm: None, status: "ns".into() },
        ];
        let text = render(&s);
        assert!(text.contains("fanwarden_cpu_temperature 41\n"));
        assert!(text.contains("fanwarden_ipmi_fan_speed{name=\"FAN1\"} 700\n"));
        assert!(text.contains("fanwarden_ipmi_fan_speed{name=\"FAN4\"} -1\n"));
        assert!(text.contains("fanwarden_bmc_up 1\n"));
    }
}
