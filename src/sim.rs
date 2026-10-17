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
    /// Hard drives warm up and cool down slowly toward a temperature set by the airflow.
    hdd_temps: Vec<f32>,
    last_step: Instant,
}

const HDD_SERIALS: [&str; 6] = ["ZCT3S9M0", "HLH01J2B", "2244E6812B23", "2244E6812A49", "ZR13LBZD", "ZCT3S7WH"];
/// Seconds for a drive to cover ~63% of the way to its new temperature.
const HDD_INERTIA_SECS: f32 = 180.0;

impl Default for Simulator {
    fn default() -> Self {
        let now = Instant::now();
        let mut sim =
            Self { start: now, mode: FanMode::Standard, duties: HashMap::new(), hdd_temps: Vec::new(), last_step: now };
        sim.hdd_temps = (0..HDD_SERIALS.len()).map(|i| sim.hdd_equilibrium(i)).collect();
        sim
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

    /// Temperature a drive settles at with the current airflow: ~1°C less per 5% of duty.
    fn hdd_equilibrium(&self, i: usize) -> f32 {
        let room = 1.5 * (self.t() / 900.0 + i as f32).sin();
        43.0 + i as f32 * 0.6 + room - 0.2 * self.duty(1)
    }

    pub fn sensors(&mut self) -> SensorData {
        let t = self.t();
        let dt = self.last_step.elapsed().as_secs_f32();
        self.last_step = Instant::now();
        for i in 0..self.hdd_temps.len() {
            let target = self.hdd_equilibrium(i);
            self.hdd_temps[i] += (target - self.hdd_temps[i]) * (dt / HDD_INERTIA_SECS).min(1.0);
        }
        let hdd = |i: usize, serial: &str| {
            let base = self.hdd_temps[i];
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
        let mut disks: Vec<DiskInfo> = HDD_SERIALS.iter().enumerate().map(|(i, s)| hdd(i, s)).collect();
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
            // Same layout as a real server: several chips share a name (one drivetemp per disk).
            hwmon: [
                ("k10temp", "0000:00:18.3", "Tctl", cpu),
                ("nvme", "nvme0", "Composite", 44.0),
                ("cxgb4_0000:85:00.4", "0000:85:00.4", "temp1", 52.0),
                ("drivetemp", "0:0:0:0", "temp1", 33.0),
                ("drivetemp", "1:0:0:0", "temp1", 35.0),
            ]
            .into_iter()
            .map(|(chip, device, label, celsius)| crate::sensors::HwmonTemp {
                chip: chip.into(),
                device: device.into(),
                label: label.into(),
                celsius,
            })
            .collect(),
            disks,
            disks_fresh: true,
            ipmi: Vec::new(),
        }
    }
}
