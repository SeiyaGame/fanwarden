//! Import of the Python version's `config.py` (fan speed grids, webhook, thresholds).

use anyhow::{Context, bail};
use regex::Regex;

use crate::config::{Config, NotificationChannel};
use crate::curve::Curve;

/// Converts a v1 grid (`(28, 30): 50` ranges and `35: 65` single temperatures, a step function)
/// into curve points: each range becomes two points with the same duty.
fn grid_to_curve(body: &str) -> anyhow::Result<Curve> {
    let entry = Regex::new(r"(?m)^\s*(?:\(\s*(-?\d+)\s*,\s*(-?\d+)\s*\)|(-?\d+))\s*:\s*(\d+)")?;
    let mut points: Vec<(f32, u8)> = Vec::new();
    for caps in entry.captures_iter(body) {
        let duty: u8 = caps[4].parse::<u8>()?.min(100);
        let temps: Vec<f32> = match (caps.get(1), caps.get(2), caps.get(3)) {
            (Some(lo), Some(hi), _) => vec![lo.as_str().parse()?, hi.as_str().parse()?],
            (_, _, Some(t)) => vec![t.as_str().parse()?],
            _ => continue,
        };
        for t in temps {
            let t = t.clamp(-20.0, 150.0);
            match points.last() {
                Some(&(last, _)) if t <= last => continue,
                _ => points.push((t, duty)),
            }
        }
    }
    if points.is_empty() {
        bail!("no entry found in the grid");
    }
    // Two points with the same duty in a row are redundant except at the ends.
    Ok(Curve::new(points))
}

fn grid<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let start = source.find(&format!("{name} = {{"))? + name.len() + 4;
    let end = source[start..].find('}')? + start;
    Some(&source[start..end])
}

fn number(source: &str, name: &str) -> Option<f32> {
    let re = Regex::new(&format!(r"(?m)^{name}\s*=\s*(-?\d+(?:\.\d+)?)")).ok()?;
    re.captures(source)?.get(1)?.as_str().parse().ok()
}

fn string(source: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"(?m)^{name}\s*=\s*["']([^"']+)["']"#)).ok()?;
    Some(re.captures(source)?.get(1)?.as_str().to_string())
}

/// Builds a configuration from a v1 `config.py`, keeping v2 defaults for everything else.
pub fn import_v1(source: &str) -> anyhow::Result<Config> {
    let mut config = Config::default();
    let disk = grid(source, "DISK_FAN_SPEED_GRID").context("DISK_FAN_SPEED_GRID not found")?;
    let cpu = grid(source, "CPU_FAN_SPEED_GRID").context("CPU_FAN_SPEED_GRID not found")?;
    for zone in &mut config.zones {
        let curve = if zone.id == 0 { grid_to_curve(cpu)? } else { grid_to_curve(disk)? };
        zone.rules[0].curve = curve;
        // v1 had no limits nor hysteresis: keep its behaviour.
        zone.min_duty = 0;
        zone.hysteresis = 0.0;
    }
    if let Some(url) = string(source, "WEBHOOK_URL") {
        config.notifications.push(NotificationChannel::Discord { enabled: true, url });
    }
    if let Some(high) = number(source, "NOTIFICATION_DISK_REACH_HIGH_TEMPERATURE") {
        config.alerts.disk_high = Some(high);
    }
    if let Some(low) = number(source, "NOTIFICATION_DISK_REACH_LOW_TEMPERATURE") {
        config.alerts.disk_low = Some(low);
    }
    if let Some(minutes) = number(source, "NOTIFICATION_SEND_EVERY_MINUTE") {
        config.alerts.repeat_minutes = minutes as u64;
    }
    config.validate().context("the imported configuration is invalid")?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = r#"
DISK_FAN_SPEED_GRID = {
    (0, 27): 40,  # Temperature from 0°C to 30°C: 40%
    (28, 30): 50,
    35: 65,
    36: 70,
    (46, 60): 100,
}

CPU_FAN_SPEED_GRID = {
    (0, 25): 30,
    (26, 34): 45,
    (81, 100): 100  # last
}

WEBHOOK_URL = "https://discord.com/api/webhooks/1/abc"
NOTIFICATION_DISK_REACH_HIGH_TEMPERATURE = 42
NOTIFICATION_DISK_REACH_LOW_TEMPERATURE = 10
NOTIFICATION_SEND_EVERY_MINUTE = 5
PROMETHEUS_ENABLE = False
"#;

    #[test]
    fn imports_grids_and_settings() {
        let config = import_v1(V1).unwrap();
        let disk = &config.zones[1].rules[0].curve;
        assert_eq!(
            disk.0,
            vec![(0.0, 40), (27.0, 40), (28.0, 50), (30.0, 50), (35.0, 65), (36.0, 70), (46.0, 100), (60.0, 100)]
        );
        // Same speeds as the v1 step function inside each range.
        assert_eq!(disk.duty_at(29.0), 50.0);
        assert_eq!(disk.duty_at(36.0), 70.0);
        assert_eq!(config.zones[0].rules[0].curve.0.len(), 6);
        assert_eq!(config.alerts.disk_high, Some(42.0));
        assert_eq!(config.alerts.repeat_minutes, 5);
        assert!(
            matches!(&config.notifications[0], NotificationChannel::Discord { url, .. } if url.ends_with("/abc"))
        );
    }

    #[test]
    fn webhook_none_is_ignored() {
        let source = V1.replace(r#""https://discord.com/api/webhooks/1/abc""#, "None");
        assert!(import_v1(&source).unwrap().notifications.is_empty());
    }

    #[test]
    fn missing_grid_is_an_error() {
        assert!(import_v1("WEBHOOK_URL = None").is_err());
    }
}
