//! Runs external tools (ipmitool, smartctl, lsblk) without a shell and with a timeout.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::process::Command;

pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub async fn run(program: &str, args: &[String], envs: &[(&str, &str)], timeout: Duration) -> anyhow::Result<Output> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    for (key, value) in envs {
        command.env(key, value);
    }
    let child = command.spawn().with_context(|| format!("cannot start {program}"))?;
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(output) => output.with_context(|| format!("{program} failed"))?,
        Err(_) => bail!("{program} timed out after {}s", timeout.as_secs()),
    };
    Ok(Output {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Like [`run`] but fails on a non-zero exit code.
pub async fn run_ok(
    program: &str,
    args: &[String],
    envs: &[(&str, &str)],
    timeout: Duration,
) -> anyhow::Result<String> {
    let output = run(program, args, envs, timeout).await?;
    if output.status != 0 {
        let detail = if output.stderr.trim().is_empty() { output.stdout.trim() } else { output.stderr.trim() };
        bail!("{program} {} exited with {}: {detail}", args.join(" "), output.status);
    }
    Ok(output.stdout)
}
