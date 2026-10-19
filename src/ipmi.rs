//! Supermicro BMC access through `ipmitool` (local /dev/ipmi0 or remote IPMI over LAN).

use std::time::Duration;

use anyhow::{Context, bail};
use serde::Serialize;

use crate::config::{FanMode, Ipmi, IpmiTarget};
use crate::exec;

const SEL_LIST_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FanReading {
    pub name: String,
    /// `None` when the BMC reports no reading (fan header not populated).
    pub rpm: Option<u32>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TempReading {
    pub name: String,
    pub celsius: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SdrReadings {
    pub fans: Vec<FanReading>,
    pub temperatures: Vec<TempReading>,
    /// Sum of the power supply input sensors (`PS1 Input Power`, `PIN`...), when the board has some.
    pub input_watts: Option<f32>,
}

#[derive(Clone)]
pub struct IpmiTool {
    config: Ipmi,
}

impl IpmiTool {
    pub fn new(config: Ipmi) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &Ipmi {
        &self.config
    }

    /// Connection arguments. The remote password goes through `IPMI_PASSWORD` (`-E`),
    /// so it never appears in the process list.
    fn base_args(&self) -> Vec<String> {
        match self.config.target {
            IpmiTarget::Local => Vec::new(),
            IpmiTarget::Remote => vec![
                "-I".into(),
                self.config.interface.clone(),
                "-H".into(),
                self.config.host.clone(),
                "-U".into(),
                self.config.username.clone(),
                "-E".into(),
            ],
        }
    }

    fn envs(&self) -> Vec<(&str, &str)> {
        match self.config.target {
            IpmiTarget::Local => Vec::new(),
            IpmiTarget::Remote => vec![("IPMI_PASSWORD", self.config.password.as_str())],
        }
    }

    async fn call(&self, args: &[&str]) -> anyhow::Result<String> {
        self.call_within(args, Duration::from_secs(self.config.timeout_secs.max(1))).await
    }

    async fn call_within(&self, args: &[&str], timeout: Duration) -> anyhow::Result<String> {
        let mut all = self.base_args();
        all.extend(args.iter().map(|a| a.to_string()));
        exec::run_ok(&self.config.ipmitool_path, &all, &self.envs(), timeout).await
    }

    async fn raw(&self, bytes: &[u8]) -> anyhow::Result<String> {
        let hex: Vec<String> = bytes.iter().map(|b| format!("0x{b:02x}")).collect();
        let mut args = vec!["raw"];
        args.extend(hex.iter().map(String::as_str));
        self.call(&args).await
    }

    pub async fn get_fan_mode(&self) -> anyhow::Result<FanMode> {
        let out = self.raw(&[0x30, 0x45, 0x00]).await?;
        let value = u8::from_str_radix(out.trim(), 16)
            .with_context(|| format!("unexpected fan mode reply '{}'", out.trim()))?;
        FanMode::from_raw(value).with_context(|| format!("unknown fan mode 0x{value:02x}"))
    }

    pub async fn set_fan_mode(&self, mode: FanMode) -> anyhow::Result<()> {
        self.raw(&[0x30, 0x45, 0x01, mode.raw()]).await?;
        // The BMC resets fan duties when the mode changes; give it a moment.
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    /// Duty cycle for a zone (X10/X11/X12/H11/H12 boards).
    pub async fn set_duty(&self, zone: u8, duty: u8) -> anyhow::Result<()> {
        if duty > 100 {
            bail!("duty {duty}% out of range");
        }
        self.raw(&[0x30, 0x70, 0x66, 0x01, zone, duty]).await?;
        Ok(())
    }

    pub async fn read_sdr(&self) -> anyhow::Result<SdrReadings> {
        Ok(parse_sdr(&self.call(&["sdr"]).await?))
    }

    pub async fn sel_info(&self) -> anyhow::Result<String> {
        self.call(&["sel", "info"]).await
    }

    /// The whole event log. `elist` looks up sensor names: slow on a full log over the network.
    pub async fn sel_list(&self) -> anyhow::Result<String> {
        self.call_within(&["sel", "elist"], SEL_LIST_TIMEOUT).await
    }

    /// Power draw from DCMI. `None` when the BMC has power readings turned off.
    pub async fn power_reading(&self) -> anyhow::Result<Option<f32>> {
        Ok(parse_dcmi_power(&self.call(&["dcmi", "power", "reading"]).await?))
    }

    /// Synchronous variant of [`set_fan_mode`], usable from a panic hook.
    pub fn set_fan_mode_blocking(&self, mode: FanMode) -> anyhow::Result<()> {
        let mut args = self.base_args();
        args.extend(["raw", "0x30", "0x45", "0x01"].map(String::from));
        args.push(format!("0x{:02x}", mode.raw()));
        let status = std::process::Command::new(&self.config.ipmitool_path)
            .args(&args)
            .envs(self.envs())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            bail!("ipmitool exited with {status}");
        }
        Ok(())
    }
}

/// Parses `ipmitool sdr` lines such as `FAN1 | 1400 RPM | ok` or `CPU Temp | 34 degrees C | ok`.
pub fn parse_sdr(text: &str) -> SdrReadings {
    let mut readings = SdrReadings::default();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        if cols.len() < 3 || cols[0].is_empty() {
            continue;
        }
        let (name, value, status) = (cols[0], cols[1], cols[cols.len() - 1]);
        let mut parts = value.split_whitespace();
        let number = parts.next().and_then(|n| n.parse::<f32>().ok());
        let unit = parts.collect::<Vec<_>>().join(" ");
        let upper = name.to_ascii_uppercase();

        if unit == "RPM" || (number.is_none() && upper.contains("FAN")) {
            readings.fans.push(FanReading {
                name: name.to_string(),
                rpm: number.filter(|_| unit == "RPM").map(|n| n.max(0.0) as u32),
                status: status.to_string(),
            });
        } else if unit == "degrees C" || (number.is_none() && upper.contains("TEMP")) {
            readings
                .temperatures
                .push(TempReading { name: name.to_string(), celsius: number.filter(|_| unit == "degrees C") });
        } else if unit == "Watts"
            && (upper.contains("INPUT") || upper.contains("PIN"))
            && let Some(watts) = number
        {
            *readings.input_watts.get_or_insert(0.0) += watts;
        }
    }
    readings
}

/// Instantaneous reading of `ipmitool dcmi power reading`.
pub fn parse_dcmi_power(text: &str) -> Option<f32> {
    if text.lines().any(|l| l.contains("Power reading state is") && l.contains("deactivated")) {
        return None;
    }
    let line = text.lines().find(|l| l.trim_start().starts_with("Instantaneous power reading"))?;
    line.split(':').nth(1)?.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDR: &str = "\
CPU Temp         | 34 degrees C      | ok
System Temp      | 31 degrees C      | ok
Peripheral Temp  | 38 degrees C      | ok
M2NVMeSSD Temp1  | no reading        | ns
FAN1             | 700 RPM           | ok
FAN2             | 1400 RPM          | ok
FAN4             | no reading        | ns
FANA             | 1000 RPM          | ok
12V              | 12.19 Volts       | ok
PS1 Status       | 0x01              | ok
PS1 Input Power  | 118 Watts         | ok
PS2 Input Power  | 6 Watts           | ok
PS1 Output Power | 101 Watts         | ok
";

    #[test]
    fn parses_fans_and_temperatures() {
        let r = parse_sdr(SDR);
        assert_eq!(r.fans.len(), 4);
        assert_eq!(r.fans[0], FanReading { name: "FAN1".into(), rpm: Some(700), status: "ok".into() });
        assert_eq!(r.fans[2].rpm, None);
        assert_eq!(r.fans[3].name, "FANA");
        assert_eq!(r.temperatures.len(), 4);
        assert_eq!(r.temperatures[0].celsius, Some(34.0));
        assert_eq!(r.temperatures[3].celsius, None);
        assert_eq!(r.input_watts, Some(124.0));
        assert_eq!(parse_sdr("FAN1 | 700 RPM | ok").input_watts, None);
    }

    #[test]
    fn parses_dcmi_power() {
        let text = "
    Instantaneous power reading:                   112 Watts
    Minimum during sampling period:                 87 Watts
    Maximum during sampling period:                245 Watts
    Average power reading over sample period:      110 Watts
    IPMI timestamp:                           Mon Oct 19 14:00:00 2026
    Sampling period:                          00000005 Seconds.
    Power reading state is:                   activated
";
        assert_eq!(parse_dcmi_power(text), Some(112.0));
        // X11 board: the timestamp and the sampling period end up on one line.
        let x11 = "
    Instantaneous power reading:                   263 Watts
    Minimum during sampling period:                258 Watts
    IPMI timestamp:                           11/28/19 22:12:42 CET    Sampling period:                          00002408 Seconds.
    Power reading state is:                   activated
";
        assert_eq!(parse_dcmi_power(x11), Some(263.0));
        assert_eq!(parse_dcmi_power(&text.replace("activated", "deactivated")), None);
        assert_eq!(parse_dcmi_power("DCMI request failed because: Invalid command (c1)"), None);
    }

    #[test]
    fn remote_args_hide_password() {
        let tool = IpmiTool::new(Ipmi {
            target: IpmiTarget::Remote,
            host: "10.0.0.5".into(),
            password: "secret".into(),
            ..Ipmi::default()
        });
        let args = tool.base_args();
        assert!(args.contains(&"-E".to_string()));
        assert!(!args.iter().any(|a| a.contains("secret")));
    }
}
