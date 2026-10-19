//! BMC backend used by the controller: real `ipmitool` or the simulator.

use std::sync::{Arc, Mutex};

use crate::config::FanMode;
use crate::ipmi::{IpmiTool, SdrReadings};
use crate::sim::Simulator;

#[derive(Clone)]
pub enum Bmc {
    Ipmi(IpmiTool),
    Sim(Arc<Mutex<Simulator>>),
}

impl Bmc {
    pub async fn get_fan_mode(&self) -> anyhow::Result<FanMode> {
        match self {
            Bmc::Ipmi(tool) => tool.get_fan_mode().await,
            Bmc::Sim(sim) => Ok(sim.lock().unwrap().mode),
        }
    }

    pub async fn set_fan_mode(&self, mode: FanMode) -> anyhow::Result<()> {
        match self {
            Bmc::Ipmi(tool) => tool.set_fan_mode(mode).await,
            Bmc::Sim(sim) => {
                sim.lock().unwrap().mode = mode;
                Ok(())
            }
        }
    }

    pub async fn set_duty(&self, zone: u8, duty: u8) -> anyhow::Result<()> {
        match self {
            Bmc::Ipmi(tool) => tool.set_duty(zone, duty).await,
            Bmc::Sim(sim) => {
                sim.lock().unwrap().set_duty(zone, duty);
                Ok(())
            }
        }
    }

    pub async fn read_sdr(&self) -> anyhow::Result<SdrReadings> {
        match self {
            Bmc::Ipmi(tool) => tool.read_sdr().await,
            Bmc::Sim(sim) => Ok(sim.lock().unwrap().sdr()),
        }
    }

    /// `ipmitool sel info` output.
    pub async fn sel_info(&self) -> anyhow::Result<String> {
        match self {
            Bmc::Ipmi(tool) => tool.sel_info().await,
            Bmc::Sim(_) => Ok(crate::sim::SEL_INFO.to_string()),
        }
    }

    /// `ipmitool sel elist` output.
    pub async fn sel_list(&self) -> anyhow::Result<String> {
        match self {
            Bmc::Ipmi(tool) => tool.sel_list().await,
            Bmc::Sim(_) => Ok(crate::sim::SEL_LIST.to_string()),
        }
    }

    pub async fn power_reading(&self) -> anyhow::Result<Option<f32>> {
        match self {
            Bmc::Ipmi(tool) => tool.power_reading().await,
            Bmc::Sim(sim) => Ok(Some(sim.lock().unwrap().power())),
        }
    }
}
