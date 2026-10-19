//! BMC hardware event log (SEL): memory errors, power supplies, intrusion, thresholds...
//! Shown in the Journal, and new warnings or failures are sent to the notification channels.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::watch;

use crate::notify::{Notification, Severity};
use crate::state::AppState;

/// `sel info` is cheap; the full list is only read again when it reports a change.
const POLL_EVERY: Duration = Duration::from_secs(60);
/// Entries kept for the UI, the most recent ones.
const KEEP: usize = 500;
/// A calibration takes fans under the BMC thresholds on purpose: no notification for fan events
/// logged meanwhile. The log is read up to a minute after the facts, hence the margin.
pub const CALIBRATION_GRACE: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SelEntry {
    pub id: u32,
    /// Date and time as the BMC reports them (its clock, often UTC).
    pub date: String,
    pub time: String,
    pub sensor: String,
    pub event: String,
    /// `Asserted` or `Deasserted`.
    pub direction: Option<String>,
    pub details: Option<String>,
    pub severity: Severity,
}

impl SelEntry {
    fn is_fan(&self) -> bool {
        self.sensor.to_ascii_lowercase().contains("fan")
    }

    pub fn summary(&self) -> String {
        let mut text = format!("{}: {}", self.sensor, self.event);
        if let Some(details) = &self.details {
            text.push_str(&format!(" ({details})"));
        }
        if self.direction.as_deref() == Some("Deasserted") {
            text.push_str(", cleared");
        }
        text
    }
}

/// What the UI shows.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BmcLog {
    /// Oldest first.
    pub entries: Vec<SelEntry>,
    pub percent_used: Option<u8>,
    pub updated: Option<u64>,
    pub error: Option<String>,
}

/// Fields of `ipmitool sel info` that change when an entry is added or the log is cleared.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelInfo {
    pub stamp: String,
    pub percent_used: Option<u8>,
}

pub fn parse_info(text: &str) -> SelInfo {
    let mut info = SelInfo::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Entries" | "Last Add Time" | "Last Del Time" => {
                info.stamp.push_str(value);
                info.stamp.push('|');
            }
            "Percent Used" => info.percent_used = value.trim_end_matches('%').parse().ok(),
            _ => {}
        }
    }
    info
}

/// Parses `ipmitool sel elist` lines such as
/// `  1a | 10/19/2026 | 14:02:11 | Memory | Correctable ECC | Asserted | CPU1/P1-DIMMA1`.
pub fn parse_list(text: &str) -> Vec<SelEntry> {
    text.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('|').map(str::trim).collect();
            if cols.len() < 5 {
                return None;
            }
            let id = u32::from_str_radix(cols[0], 16).ok()?;
            let optional = |s: Option<&&str>| s.filter(|s| !s.is_empty()).map(|s| s.to_string());
            let details = cols[6..].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
            let mut entry = SelEntry {
                id,
                date: cols[1].to_string(),
                time: cols[2].to_string(),
                sensor: cols[3].to_string(),
                event: cols[4].to_string(),
                direction: optional(cols.get(5)),
                details: (!details.is_empty()).then_some(details),
                severity: Severity::Info,
            };
            entry.severity = classify(&entry);
            Some(entry)
        })
        .collect()
}

/// How serious an event is, from its sensor and description.
pub fn classify(entry: &SelEntry) -> Severity {
    if entry.direction.as_deref() == Some("Deasserted") {
        return Severity::Info;
    }
    let text = format!("{} {}", entry.sensor, entry.event).to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&[
        "uncorrectable",
        "non-recoverable",
        "failure detected",
        "predictive failure",
        "ac lost",
        "power supply failure",
        "ierr",
        "thermal trip",
        "machine check",
        "fatal",
        "drive fault",
    ]) || (text.contains("critical going") && !text.contains("non-critical going"))
    {
        Severity::Critical
    } else if has(&[
        "correctable ecc",
        "non-critical going",
        "intrusion",
        "watchdog",
        "throttl",
        "configuration error",
        "redundancy lost",
        "redundancy degraded",
        "out of range",
    ]) {
        Severity::Warning
    } else {
        Severity::Info
    }
}

/// Finds the entries added since the previous reading. The first reading is the baseline.
#[derive(Debug, Default)]
pub struct Tracker {
    seen: Option<(usize, u32)>,
}

impl Tracker {
    pub fn new_entries(&mut self, entries: &[SelEntry]) -> Vec<SelEntry> {
        let max_id = entries.iter().map(|e| e.id).max().unwrap_or(0);
        let previous = self.seen.replace((entries.len(), max_id));
        match previous {
            None => Vec::new(),
            // Fewer entries: the log was cleared, everything in it is new.
            Some((count, _)) if entries.len() < count => entries.to_vec(),
            Some((_, last_id)) => entries.iter().filter(|e| e.id > last_id).cloned().collect(),
        }
    }
}

/// Reads the BMC log every minute, publishes it and notifies new warnings and failures.
pub async fn task(state: Arc<AppState>, mut shutdown: watch::Receiver<bool>) {
    let mut tracker = Tracker::default();
    let mut last_stamp: Option<String> = None;
    loop {
        let bmc = state.bmc.read().unwrap().clone();
        if let Some(bmc) = bmc {
            match read(&bmc, last_stamp.as_deref()).await {
                Ok(Some((info, entries))) => {
                    last_stamp = Some(info.stamp);
                    let fresh = tracker.new_entries(&entries);
                    let start = entries.len().saturating_sub(KEEP);
                    state.set_bmc_log(BmcLog {
                        entries: entries[start..].to_vec(),
                        percent_used: info.percent_used,
                        updated: Some(crate::now()),
                        error: None,
                    });
                    report(&state, fresh).await;
                }
                Ok(None) => state.update_bmc_log(|log| {
                    log.updated = Some(crate::now());
                    log.error = None;
                }),
                Err(e) => {
                    let message = format!("{e:#}");
                    if state.bmc_log().error.as_ref() != Some(&message) {
                        tracing::warn!("cannot read the BMC event log: {message}");
                    }
                    state.update_bmc_log(|log| log.error = Some(message));
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(POLL_EVERY) => {}
            _ = shutdown.changed() => return,
        }
    }
}

/// The log, unless `sel info` says nothing changed since `last_stamp`.
async fn read(bmc: &crate::bmc::Bmc, last_stamp: Option<&str>) -> anyhow::Result<Option<(SelInfo, Vec<SelEntry>)>> {
    let info = parse_info(&bmc.sel_info().await?);
    if last_stamp == Some(info.stamp.as_str()) {
        return Ok(None);
    }
    let entries = parse_list(&bmc.sel_list().await?);
    Ok(Some((info, entries)))
}

async fn report(state: &AppState, fresh: Vec<SelEntry>) {
    let config = state.config();
    let paused = state.calibrated_within(CALIBRATION_GRACE);
    let mut notifications = Vec::new();
    for entry in fresh {
        let text = entry.summary();
        if entry.severity == Severity::Info || (paused && entry.is_fan()) {
            tracing::info!("BMC event: {text}");
            continue;
        }
        tracing::warn!("BMC event: {text}");
        if config.alerts.bmc_events {
            notifications.push(Notification { title: "BMC event".into(), message: text, severity: entry.severity });
        }
    }
    for notification in notifications {
        state.notifier.broadcast(&config.notifications, &notification).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "\
SEL Information
Version          : 1.5 (v1.5, v2 compliant)
Entries          : 4
Free Space       : 10688 bytes
Percent Used     : 3%
Last Add Time    : 10/19/2026 14:05:00
Last Del Time    : Not Available
Overflow         : false
";

    const LIST: &str = "\
   1 | 10/18/2026 | 09:12:44 | System Boot Initiated #0x00 | Initiated by power up | Asserted
   2 | 10/19/2026 | 14:02:11 | Power Supply PS2 Status | Power Supply AC lost | Asserted
   3 | 10/19/2026 | 14:02:40 | Power Supply PS2 Status | Power Supply AC lost | Deasserted
   a | 10/19/2026 | 14:05:00 | Memory | Correctable ECC | Asserted | CPU1/P1-DIMMA1
   b | 10/19/2026 | 14:06:00 | Fan FANA | Lower Critical going low  | Asserted | Reading 100 < Threshold 200 RPM
   c | 10/19/2026 | 14:07:00 | Temperature CPU Temp | Upper Non-critical going high | Asserted
   d | Pre-Init  | 0000000012 | Physical Security #0xaa | General Chassis intrusion | Asserted
";

    #[test]
    fn parses_info() {
        let info = parse_info(INFO);
        assert_eq!(info.percent_used, Some(3));
        assert_eq!(info.stamp, "4|10/19/2026 14:05:00|Not Available|");
        assert_eq!(parse_info("Percent Used : unknown").percent_used, None);
    }

    #[test]
    fn parses_and_classifies_entries() {
        let entries = parse_list(LIST);
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[3].id, 10);
        assert_eq!(entries[3].details.as_deref(), Some("CPU1/P1-DIMMA1"));
        assert_eq!(entries[0].details, None);
        let severities: Vec<Severity> = entries.iter().map(|e| e.severity).collect();
        use Severity::*;
        assert_eq!(severities, [Info, Critical, Info, Warning, Critical, Warning, Warning]);
        assert_eq!(entries[2].summary(), "Power Supply PS2 Status: Power Supply AC lost, cleared");
        assert_eq!(entries[4].summary(), "Fan FANA: Lower Critical going low (Reading 100 < Threshold 200 RPM)");
    }

    #[test]
    fn ignores_headers_and_garbage() {
        assert!(parse_list("SEL has no entries\n\n zz | a | b | c | d").is_empty());
    }

    #[test]
    fn tracker_reports_only_new_entries() {
        let entries = parse_list(LIST);
        let mut tracker = Tracker::default();
        assert!(tracker.new_entries(&entries[..4]).is_empty(), "the first reading is the baseline");
        assert!(tracker.new_entries(&entries[..4]).is_empty());
        let fresh = tracker.new_entries(&entries);
        assert_eq!(fresh.iter().map(|e| e.id).collect::<Vec<_>>(), [11, 12, 13]);
        // Cleared, then one event logged: it is new although its id was seen before.
        let cleared = parse_list("   1 | 10/20/2026 | 08:00:00 | Memory | Uncorrectable ECC | Asserted");
        assert_eq!(tracker.new_entries(&cleared).len(), 1);
    }
}
