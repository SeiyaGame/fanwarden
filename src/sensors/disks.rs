//! Disk temperatures and health through `lsblk` (inventory) and `smartctl --json` (SMART).

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Disks;
use crate::exec;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiskKind {
    Hdd,
    Ssd,
    Nvme,
}

impl DiskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DiskKind::Hdd => "hdd",
            DiskKind::Ssd => "ssd",
            DiskKind::Nvme => "nvme",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiskState {
    Active,
    Standby,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiskInfo {
    pub name: String,
    pub serial: String,
    pub model: String,
    pub kind: DiskKind,
    pub state: DiskState,
    pub temperature: Option<f32>,
    pub error: Option<String>,
    /// Last SMART health read. Kept from a previous scan while the disk sleeps.
    pub health: Option<DiskHealth>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    #[default]
    Ok,
    /// Worn or damaged media: reallocated or pending sectors, NVMe media errors or wear.
    Warning,
    /// The drive's own self-assessment failed.
    Failing,
}

/// SMART counters that tell whether a drive is wearing out. `None`: not reported by this drive.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiskHealth {
    pub status: HealthStatus,
    /// Overall SMART self-assessment (`smartctl -H`).
    pub passed: Option<bool>,
    pub power_on_hours: Option<u64>,
    /// ATA 5, or the grown defect list of SAS drives.
    pub reallocated: Option<u64>,
    /// ATA 197: sectors waiting to be remapped.
    pub pending: Option<u64>,
    /// ATA 199: transfer errors, usually a bad cable or backplane.
    pub crc_errors: Option<u64>,
    /// NVMe.
    pub media_errors: Option<u64>,
    /// NVMe: share of the rated endurance used, can exceed 100.
    pub percentage_used: Option<u64>,
}

/// NVMe wear from which a drive is flagged.
const WEAR_WARNING_PERCENT: u64 = 90;

impl DiskHealth {
    fn assess(&self, nvme_critical_warning: bool) -> HealthStatus {
        let positive = |v: Option<u64>| v.is_some_and(|v| v > 0);
        if self.passed == Some(false) || nvme_critical_warning {
            HealthStatus::Failing
        } else if positive(self.reallocated)
            || positive(self.pending)
            || positive(self.media_errors)
            || self.percentage_used.is_some_and(|p| p >= WEAR_WARNING_PERCENT)
        {
            HealthStatus::Warning
        } else {
            HealthStatus::Ok
        }
    }
}

/// A sleeping disk reports nothing: keep the health of the previous scan.
pub fn keep_known_health(previous: &[DiskInfo], disks: &mut [DiskInfo]) {
    for disk in disks.iter_mut().filter(|d| d.health.is_none()) {
        disk.health = previous.iter().find(|p| p.serial == disk.serial).and_then(|p| p.health.clone());
    }
}

/// A block device listed by lsblk.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDevice {
    pub name: String,
    pub kind: DiskKind,
    pub serial: Option<String>,
    pub model: Option<String>,
}

pub async fn scan(config: &Disks) -> anyhow::Result<Vec<DiskInfo>> {
    let exclude = regex::Regex::new(&config.exclude)?;
    let args: Vec<String> = ["-J", "-d", "-o", "NAME,ROTA,TRAN,TYPE,MODEL,SERIAL"].map(String::from).to_vec();
    let out = exec::run_ok(&config.lsblk_path, &args, &[], Duration::from_secs(10)).await?;
    let devices: Vec<BlockDevice> = parse_lsblk(&out)?
        .into_iter()
        .filter(|d| config.exclude.is_empty() || !exclude.is_match(&d.name))
        // lsblk reads /sys and lists every disk of the host; in a container that only received
        // some devices (`devices:` in compose), skip the ones without a /dev node.
        .filter(|d| std::path::Path::new("/dev").join(&d.name).exists())
        .collect();

    let reads = devices.into_iter().map(|device| async move {
        match smartctl(config, &device.name).await {
            Ok(smart) => smart.into_info(device),
            Err(e) => DiskInfo {
                serial: device.serial.clone().unwrap_or_else(|| device.name.clone()),
                model: device.model.clone().unwrap_or_default(),
                name: device.name,
                kind: device.kind,
                state: DiskState::Error,
                temperature: None,
                error: Some(format!("{e:#}")),
                health: None,
            },
        }
    });
    Ok(futures_util::future::join_all(reads).await)
}

async fn smartctl(config: &Disks, name: &str) -> anyhow::Result<Smart> {
    let mut args: Vec<String> = ["--json", "-i", "-H", "-A"].map(String::from).to_vec();
    if config.skip_standby {
        args.extend(["-n", "standby"].map(String::from));
    }
    args.push(format!("/dev/{name}"));
    // smartctl's exit code is a bit mask that is often non-zero on healthy disks: rely on the JSON.
    let out = exec::run(&config.smartctl_path, &args, &[], Duration::from_secs(20)).await?;
    parse_smartctl(&out.stdout).with_context(|| format!("smartctl /dev/{name}: {}", out.stderr.trim()))
}

pub fn parse_lsblk(json: &str) -> anyhow::Result<Vec<BlockDevice>> {
    let root: Value = serde_json::from_str(json).context("invalid lsblk output")?;
    let devices = root["blockdevices"].as_array().context("lsblk: no blockdevices")?;
    let text = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    Ok(devices
        .iter()
        .filter(|d| d["type"].as_str() == Some("disk"))
        .filter_map(|d| {
            let name = text(&d["name"])?;
            // Depending on the lsblk version ROTA is a boolean, "1"/"0" or 1/0.
            let rotational = match &d["rota"] {
                Value::Bool(b) => *b,
                Value::String(s) => s.trim() == "1",
                Value::Number(n) => n.as_u64() == Some(1),
                _ => false,
            };
            let kind = if name.starts_with("nvme") || d["tran"].as_str() == Some("nvme") {
                DiskKind::Nvme
            } else if rotational {
                DiskKind::Hdd
            } else {
                DiskKind::Ssd
            };
            Some(BlockDevice { name, kind, serial: text(&d["serial"]), model: text(&d["model"]) })
        })
        .collect())
}

#[derive(Debug, Default, PartialEq)]
pub struct Smart {
    pub standby: bool,
    pub temperature: Option<f32>,
    pub serial: Option<String>,
    pub model: Option<String>,
    pub health: Option<DiskHealth>,
}

impl Smart {
    fn into_info(self, device: BlockDevice) -> DiskInfo {
        let state = if self.standby {
            DiskState::Standby
        } else if self.temperature.is_some() {
            DiskState::Active
        } else {
            DiskState::Error
        };
        DiskInfo {
            serial: self.serial.or(device.serial).unwrap_or_else(|| device.name.clone()),
            model: self.model.or(device.model).unwrap_or_default(),
            error: (state == DiskState::Error).then(|| "no temperature reported".to_string()),
            name: device.name,
            kind: device.kind,
            state,
            temperature: self.temperature,
            health: self.health,
        }
    }
}

pub fn parse_smartctl(json: &str) -> anyhow::Result<Smart> {
    let root: Value = serde_json::from_str(json).context("invalid smartctl JSON")?;
    let standby = root["smartctl"]["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|m| {
            let s = m["string"].as_str().unwrap_or_default().to_ascii_uppercase();
            s.contains("STANDBY") || s.contains("SLEEP")
        })
    });
    let text = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let temperature = root["temperature"]["current"]
        .as_f64()
        .or_else(|| root["nvme_smart_health_information_log"]["temperature"].as_f64())
        .or_else(|| ata_attribute(&root, 194))
        .or_else(|| ata_attribute(&root, 190))
        .map(|t| t as f32);
    Ok(Smart {
        standby,
        temperature: if standby { None } else { temperature },
        serial: text(&root["serial_number"]),
        model: text(&root["model_name"]).or_else(|| text(&root["scsi_model_name"])),
        health: if standby { None } else { parse_health(&root) },
    })
}

fn parse_health(root: &Value) -> Option<DiskHealth> {
    let nvme = &root["nvme_smart_health_information_log"];
    let mut health = DiskHealth {
        status: HealthStatus::Ok,
        passed: root["smart_status"]["passed"].as_bool(),
        power_on_hours: root["power_on_time"]["hours"].as_u64().or_else(|| nvme["power_on_hours"].as_u64()),
        reallocated: ata_raw(root, 5).or_else(|| root["scsi_grown_defect_list"].as_u64()),
        pending: ata_raw(root, 197),
        crc_errors: ata_raw(root, 199),
        media_errors: nvme["media_errors"].as_u64(),
        percentage_used: nvme["percentage_used"].as_u64(),
    };
    if health == DiskHealth::default() {
        return None;
    }
    health.status = health.assess(nvme["critical_warning"].as_u64().is_some_and(|w| w != 0));
    Some(health)
}

/// Raw value of an ATA SMART attribute.
fn ata_raw(root: &Value, id: u64) -> Option<u64> {
    root["ata_smart_attributes"]["table"]
        .as_array()?
        .iter()
        .find(|a| a["id"].as_u64() == Some(id))
        .and_then(|a| a["raw"]["value"].as_u64())
}

/// Temperature from an ATA SMART attribute (the low byte of the raw value).
fn ata_attribute(root: &Value, id: u64) -> Option<f64> {
    ata_raw(root, id).map(|raw| (raw & 0xff) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsblk_both_rota_formats() {
        let json = r#"{"blockdevices":[
            {"name":"sda","rota":true,"tran":"sata","type":"disk","model":"ST8000VN004","serial":"ZCT3S9M0"},
            {"name":"sdb","rota":"0","tran":"sata","type":"disk","model":"Samsung SSD","serial":"S1"},
            {"name":"nvme0n1","rota":false,"tran":"nvme","type":"disk","model":"WD","serial":"N1"},
            {"name":"sr0","rota":true,"tran":"sata","type":"rom","model":null,"serial":null}
        ]}"#;
        let devices = parse_lsblk(json).unwrap();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].kind, DiskKind::Hdd);
        assert_eq!(devices[1].kind, DiskKind::Ssd);
        assert_eq!(devices[2].kind, DiskKind::Nvme);
        assert_eq!(devices[0].serial.as_deref(), Some("ZCT3S9M0"));
    }

    #[test]
    fn smartctl_ata() {
        let json = r#"{"smartctl":{"exit_status":4},"model_name":"ST8000VN004-2M2101","serial_number":"ZCT3S9M0",
            "temperature":{"current":32}}"#;
        let smart = parse_smartctl(json).unwrap();
        assert_eq!(smart.temperature, Some(32.0));
        assert_eq!(smart.serial.as_deref(), Some("ZCT3S9M0"));
        assert!(!smart.standby);
    }

    #[test]
    fn smartctl_attribute_fallback() {
        let json = r#"{"ata_smart_attributes":{"table":[{"id":194,"raw":{"value":309237645348}}]}}"#;
        assert_eq!(parse_smartctl(json).unwrap().temperature, Some(36.0));
    }

    #[test]
    fn smartctl_standby() {
        let json = r#"{"smartctl":{"exit_status":2,"messages":[{"string":"Device is in STANDBY mode, exit(2)","severity":"information"}]}}"#;
        let smart = parse_smartctl(json).unwrap();
        assert!(smart.standby);
        assert_eq!(smart.temperature, None);
    }

    #[test]
    fn smartctl_nvme() {
        let json = r#"{"model_name":"WD Red SN700","serial_number":"N1","nvme_smart_health_information_log":{"temperature":41}}"#;
        assert_eq!(parse_smartctl(json).unwrap().temperature, Some(41.0));
    }

    #[test]
    fn health_of_a_healthy_ata_disk() {
        let json = r#"{"smart_status":{"passed":true},"power_on_time":{"hours":26543},
            "ata_smart_attributes":{"table":[
                {"id":5,"raw":{"value":0}},{"id":197,"raw":{"value":0}},{"id":199,"raw":{"value":3}}]}}"#;
        let health = parse_smartctl(json).unwrap().health.unwrap();
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.passed, Some(true));
        assert_eq!(health.power_on_hours, Some(26543));
        assert_eq!((health.reallocated, health.pending, health.crc_errors), (Some(0), Some(0), Some(3)));
        assert_eq!(health.media_errors, None);
    }

    #[test]
    fn health_warns_on_bad_sectors_and_fails_on_the_self_assessment() {
        let json =
            r#"{"smart_status":{"passed":true},"ata_smart_attributes":{"table":[{"id":197,"raw":{"value":2}}]}}"#;
        assert_eq!(parse_smartctl(json).unwrap().health.unwrap().status, HealthStatus::Warning);

        let json = r#"{"smart_status":{"passed":false}}"#;
        assert_eq!(parse_smartctl(json).unwrap().health.unwrap().status, HealthStatus::Failing);
    }

    #[test]
    fn health_of_nvme_and_sas() {
        let json = r#"{"smart_status":{"passed":true},"nvme_smart_health_information_log":
            {"critical_warning":0,"media_errors":0,"percentage_used":93,"power_on_hours":9100}}"#;
        let health = parse_smartctl(json).unwrap().health.unwrap();
        assert_eq!((health.status, health.power_on_hours), (HealthStatus::Warning, Some(9100)));

        let json = r#"{"nvme_smart_health_information_log":{"critical_warning":4,"percentage_used":3}}"#;
        assert_eq!(parse_smartctl(json).unwrap().health.unwrap().status, HealthStatus::Failing);

        let json = r#"{"smart_status":{"passed":true},"scsi_grown_defect_list":12}"#;
        let health = parse_smartctl(json).unwrap().health.unwrap();
        assert_eq!((health.status, health.reallocated), (HealthStatus::Warning, Some(12)));
    }

    #[test]
    fn no_health_while_asleep_or_unreported() {
        let json =
            r#"{"smartctl":{"messages":[{"string":"Device is in STANDBY mode"}]},"smart_status":{"passed":true}}"#;
        assert_eq!(parse_smartctl(json).unwrap().health, None);
        assert_eq!(parse_smartctl(r#"{"temperature":{"current":30}}"#).unwrap().health, None);
    }

    #[test]
    fn sleeping_disk_keeps_its_previous_health() {
        let disk = |serial: &str, health: Option<DiskHealth>| DiskInfo {
            name: "sda".into(),
            serial: serial.into(),
            model: String::new(),
            kind: DiskKind::Hdd,
            state: DiskState::Standby,
            temperature: None,
            error: None,
            health,
        };
        let known = DiskHealth { passed: Some(true), reallocated: Some(4), ..DiskHealth::default() };
        let previous = [disk("S1", Some(known.clone()))];
        let mut now = [disk("S1", None), disk("S2", None)];
        keep_known_health(&previous, &mut now);
        assert_eq!(now[0].health, Some(known));
        assert_eq!(now[1].health, None);
    }
}
