//! Temperature sources: CPU and other hwmon chips, disks (SMART) and BMC sensors.

pub mod disks;
pub mod hwmon;

use anyhow::bail;
use serde::Serialize;

use crate::ipmi::TempReading;
pub use disks::{DiskInfo, DiskKind, DiskState};
pub use hwmon::HwmonTemp;

/// Everything read from the sensors during one iteration.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SensorData {
    pub cpu: Option<f32>,
    pub hwmon: Vec<HwmonTemp>,
    pub disks: Vec<DiskInfo>,
    /// False when the last successful disk scan is too old to be trusted.
    pub disks_fresh: bool,
    pub ipmi: Vec<TempReading>,
}

/// A temperature source referenced by a zone rule, written as a string in the config:
/// `cpu`, `disks:all|hdd|ssd|nvme`, `disk:<serial>`, `hwmon:<chip>[@<device>]/<label>`, `ipmi:<sensor name>`.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Cpu,
    Disks(Option<DiskKind>),
    Disk(String),
    Hwmon { chip: String, device: Option<String>, label: String },
    Ipmi(String),
}

/// Result of reading a source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reading {
    Temp(f32),
    /// Nothing to cool: every matching disk is spun down.
    Idle,
    /// The source cannot be read: the zone goes to its failsafe duty.
    Missing,
}

impl Source {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let (kind, arg) = text.split_once(':').unwrap_or((text, ""));
        Ok(match (kind, arg) {
            ("cpu", "") => Source::Cpu,
            ("disks", "all") => Source::Disks(None),
            ("disks", kind) => Source::Disks(Some(match kind {
                "hdd" => DiskKind::Hdd,
                "ssd" => DiskKind::Ssd,
                "nvme" => DiskKind::Nvme,
                _ => bail!("unknown disk kind '{kind}' (all, hdd, ssd, nvme)"),
            })),
            ("disk", serial) if !serial.is_empty() => Source::Disk(serial.to_string()),
            ("hwmon", path) => match path.split_once('/') {
                Some((chip, label)) if !chip.is_empty() && !label.is_empty() => {
                    let (chip, device) = match chip.split_once('@') {
                        Some((chip, device)) => (chip, Some(device.to_string())),
                        None => (chip, None),
                    };
                    Source::Hwmon { chip: chip.to_string(), device, label: label.to_string() }
                }
                _ => bail!("expected hwmon:<chip>[@<device>]/<label>"),
            },
            ("ipmi", name) if !name.is_empty() => Source::Ipmi(name.to_string()),
            _ => bail!("unknown source '{text}'"),
        })
    }

    pub fn read(&self, data: &SensorData) -> Reading {
        let found = |value: Option<f32>| value.map_or(Reading::Missing, Reading::Temp);
        match self {
            Source::Cpu => found(data.cpu),
            Source::Hwmon { chip, device, label } => found(
                data.hwmon
                    .iter()
                    .find(|h| &h.chip == chip && &h.label == label && device.as_ref().is_none_or(|d| &h.device == d))
                    .map(|h| h.celsius),
            ),
            Source::Ipmi(name) => found(data.ipmi.iter().find(|t| &t.name == name).and_then(|t| t.celsius)),
            Source::Disk(serial) => {
                if !data.disks_fresh {
                    return Reading::Missing;
                }
                disks_reading(data.disks.iter().filter(|d| &d.serial == serial))
            }
            Source::Disks(kind) => {
                if !data.disks_fresh {
                    return Reading::Missing;
                }
                disks_reading(data.disks.iter().filter(|d| kind.is_none_or(|k| d.kind == k)))
            }
        }
    }
}

fn disks_reading<'a>(disks: impl Iterator<Item = &'a DiskInfo>) -> Reading {
    let mut hottest: Option<f32> = None;
    let mut any_standby = false;
    for disk in disks {
        match (disk.state, disk.temperature) {
            (DiskState::Active, Some(t)) => hottest = Some(hottest.map_or(t, |h| h.max(t))),
            (DiskState::Standby, _) => any_standby = true,
            _ => {}
        }
    }
    match hottest {
        Some(t) => Reading::Temp(t),
        None if any_standby => Reading::Idle,
        None => Reading::Missing,
    }
}

/// A source the UI can offer, with its current value.
#[derive(Debug, Clone, Serialize)]
pub struct SourceOption {
    pub id: String,
    pub label: String,
    pub celsius: Option<f32>,
}

pub fn available_sources(data: &SensorData) -> Vec<SourceOption> {
    let mut options = Vec::new();
    let mut push = |id: String, label: String| {
        let celsius = match Source::parse(&id).map(|s| s.read(data)) {
            Ok(Reading::Temp(t)) => Some(t),
            _ => None,
        };
        options.push(SourceOption { id, label, celsius });
    };
    push("cpu".into(), "CPU (package)".into());
    push("disks:all".into(), "Hottest disk".into());
    for (kind, label) in
        [(DiskKind::Hdd, "Hottest HDD"), (DiskKind::Ssd, "Hottest SSD"), (DiskKind::Nvme, "Hottest NVMe")]
    {
        if data.disks.iter().any(|d| d.kind == kind) {
            push(format!("disks:{}", kind.as_str()), label.into());
        }
    }
    for disk in &data.disks {
        push(format!("disk:{}", disk.serial), format!("Disk {} ({})", disk.name, disk.serial));
    }
    for t in &data.ipmi {
        push(format!("ipmi:{}", t.name), format!("BMC {}", t.name));
    }
    // drivetemp exposes the disks already covered by SMART: not offered twice.
    for h in data.hwmon.iter().filter(|h| h.chip != "drivetemp") {
        let id = h.source_id(&data.hwmon);
        let label = if id.contains('@') {
            format!("{} {} ({})", h.chip, h.label, h.device)
        } else {
            format!("{} {}", h.chip, h.label)
        };
        push(id, label);
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(serial: &str, kind: DiskKind, state: DiskState, temp: Option<f32>) -> DiskInfo {
        DiskInfo {
            name: serial.to_lowercase(),
            serial: serial.into(),
            model: String::new(),
            kind,
            state,
            temperature: temp,
            error: None,
        }
    }

    fn data() -> SensorData {
        SensorData {
            cpu: Some(42.0),
            disks: vec![
                disk("A", DiskKind::Hdd, DiskState::Active, Some(35.0)),
                disk("B", DiskKind::Hdd, DiskState::Active, Some(38.0)),
                disk("C", DiskKind::Nvme, DiskState::Active, Some(50.0)),
                disk("D", DiskKind::Ssd, DiskState::Standby, None),
            ],
            disks_fresh: true,
            ipmi: vec![TempReading { name: "System Temp".into(), celsius: Some(30.0) }],
            ..SensorData::default()
        }
    }

    #[test]
    fn parse_sources() {
        assert_eq!(Source::parse("cpu").unwrap(), Source::Cpu);
        assert_eq!(Source::parse("disks:hdd").unwrap(), Source::Disks(Some(DiskKind::Hdd)));
        assert_eq!(Source::parse("disks:all").unwrap(), Source::Disks(None));
        assert_eq!(Source::parse("ipmi:CPU Temp").unwrap(), Source::Ipmi("CPU Temp".into()));
        assert_eq!(
            Source::parse("hwmon:nvme/Composite").unwrap(),
            Source::Hwmon { chip: "nvme".into(), device: None, label: "Composite".into() }
        );
        assert!(Source::parse("disks:tape").is_err());
        assert!(Source::parse("gpu").is_err());
    }

    #[test]
    fn read_sources() {
        let d = data();
        assert_eq!(Source::Cpu.read(&d), Reading::Temp(42.0));
        assert_eq!(Source::Disks(Some(DiskKind::Hdd)).read(&d), Reading::Temp(38.0));
        assert_eq!(Source::Disks(None).read(&d), Reading::Temp(50.0));
        assert_eq!(Source::Disks(Some(DiskKind::Ssd)).read(&d), Reading::Idle);
        assert_eq!(Source::Disk("A".into()).read(&d), Reading::Temp(35.0));
        assert_eq!(Source::Disk("Z".into()).read(&d), Reading::Missing);
        assert_eq!(Source::Ipmi("System Temp".into()).read(&d), Reading::Temp(30.0));
    }

    #[test]
    fn hwmon_ids_are_unique_and_resolvable() {
        let t = |chip: &str, device: &str, label: &str, c: f32| HwmonTemp {
            chip: chip.into(),
            device: device.into(),
            label: label.into(),
            celsius: c,
        };
        let mut d = data();
        d.hwmon = vec![
            t("k10temp", "0000:00:18.3", "Tctl", 45.0),
            t("cxgb4_0000:85:00.4", "0000:85:00.4", "temp1", 50.0),
            t("nvme", "nvme0", "Composite", 40.0),
            t("nvme", "nvme1", "Composite", 42.0),
            t("drivetemp", "0:0:0:0", "temp1", 33.0),
            t("drivetemp", "1:0:0:0", "temp1", 35.0),
        ];
        let options = available_sources(&d);
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate source ids: {ids:?}");
        assert!(ids.contains(&"hwmon:k10temp/Tctl"));
        assert!(ids.contains(&"hwmon:nvme@nvme1/Composite"));
        assert!(!ids.iter().any(|id| id.contains("drivetemp")));

        let read = |id: &str| Source::parse(id).unwrap().read(&d);
        assert_eq!(read("hwmon:nvme@nvme1/Composite"), Reading::Temp(42.0));
        assert_eq!(read("hwmon:cxgb4_0000:85:00.4/temp1"), Reading::Temp(50.0));
        // Without a device, the first chip matching the name.
        assert_eq!(read("hwmon:nvme/Composite"), Reading::Temp(40.0));
    }

    #[test]
    fn stale_disks_are_missing() {
        let mut d = data();
        d.disks_fresh = false;
        assert_eq!(Source::Disks(None).read(&d), Reading::Missing);
    }
}
