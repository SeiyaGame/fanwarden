//! Simulated server (`--simulate`): fake BMC and sensors reacting to fan duty,
//! to try the UI and the control loop on a machine without IPMI.

use std::collections::HashMap;
use std::time::Instant;

use crate::config::FanMode;
use crate::ipmi::{FanReading, SdrReadings, TempReading};
use crate::sensors::{DiskInfo, DiskKind, DiskState, SensorData};

pub struct Simulator {
    start: Instant,
    pub mode: FanMode,
    duties: HashMap<u8, u8>,
}

impl Default for Simulator {
    fn default() -> Self {
        Self { start: Instant::now(), mode: FanMode::Standard, duties: HashMap::new() }
    }
}

impl Simulator {
    pub fn set_duty(&mut self, zone: u8, duty: u8) {
        self.duties.insert(zone, duty);
    }

    fn duty(&self, zone: u8) -> f32 {
        if self.mode == FanMode::Full { self.duties.get(&zone).copied().unwrap_or(100) as f32 } else { 50.0 }
    }

    fn t(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn cpu(&self) -> f32 {
        let t = self.t();
        let load = 48.0 + 14.0 * (t / 90.0).sin() + 3.0 * (t / 7.0).sin();
        load - (self.duty(0) - 40.0) * 0.18
    }

    pub fn sdr(&self) -> SdrReadings {
        let (cpu_duty, periph_duty) = (self.duty(0), self.duty(1));
        let rpm = |duty: f32, offset: f32| Some((300.0 + duty * 15.0 + offset) as u32);
        let fan = |name: &str, rpm: Option<u32>| FanReading {
            name: name.into(),
            rpm,
            status: if rpm.is_some() { "ok".into() } else { "ns".into() },
        };
        let temp = |name: &str, c: f32| TempReading { name: name.into(), celsius: Some(c.round()) };
        SdrReadings {
            fans: vec![
                fan("FAN1", rpm(cpu_duty, 0.0)),
                fan("FAN2", rpm(cpu_duty, 40.0)),
                fan("FAN3", rpm(cpu_duty, -30.0)),
                fan("FAN4", None),
                fan("FANA", rpm(periph_duty, 100.0)),
                fan("FANB", rpm(periph_duty, 60.0)),
            ],
            temperatures: vec![
                temp("CPU Temp", self.cpu()),
                temp("System Temp", 29.0 + (self.t() / 300.0).sin() * 2.0),
                temp("Peripheral Temp", 36.0 - (periph_duty - 50.0) * 0.05),
            ],
        }
    }

    pub fn sensors(&self) -> SensorData {
        let t = self.t();
        let periph = self.duty(1);
        let hdd = |i: usize, serial: &str| {
            let base = 33.0 + i as f32 * 0.8 + 4.0 * (t / 240.0 + i as f32).sin() - (periph - 50.0) * 0.06;
            DiskInfo {
                name: format!("sd{}", (b'a' + i as u8) as char),
                serial: serial.into(),
                model: "ST8000VN004-2M2101".into(),
                kind: DiskKind::Hdd,
                state: DiskState::Active,
                temperature: Some(base.round()),
                error: None,
            }
        };
        let mut disks: Vec<DiskInfo> = ["ZCT3S9M0", "HLH01J2B", "2244E6812B23", "2244E6812A49", "ZR13LBZD", "ZCT3S7WH"]
            .iter()
            .enumerate()
            .map(|(i, s)| hdd(i, s))
            .collect();
        disks.push(DiskInfo {
            name: "sdg".into(),
            serial: "S4EVNX0R".into(),
            model: "Samsung SSD 870 EVO".into(),
            kind: DiskKind::Ssd,
            state: DiskState::Standby,
            temperature: None,
            error: None,
        });
        disks.push(DiskInfo {
            name: "nvme0n1".into(),
            serial: "22411K800123".into(),
            model: "WD Red SN700 1000GB".into(),
            kind: DiskKind::Nvme,
            state: DiskState::Active,
            temperature: Some((44.0 + 5.0 * (t / 60.0).sin()).round()),
            error: None,
        });
        let cpu = self.cpu();
        SensorData {
            cpu: Some((cpu * 10.0).round() / 10.0),
            hwmon: vec![crate::sensors::HwmonTemp { chip: "k10temp".into(), label: "Tctl".into(), celsius: cpu }],
            disks,
            disks_fresh: true,
            ipmi: Vec::new(),
        }
    }
}
