//! Fan control for Supermicro motherboards through IPMI, with a web UI.

pub mod alerts;
pub mod bmc;
pub mod config;
pub mod controller;
pub mod curve;
pub mod events;
pub mod exec;
pub mod history;
pub mod ipmi;
pub mod notify;
pub mod sensors;
pub mod sim;
pub mod state;
pub mod web;

/// Current Unix time in seconds.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
