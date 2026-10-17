# fan-control

Fan control for Supermicro motherboards through IPMI, driven by CPU and disk temperatures, with a web UI.

A single static binary: no Python, no runtime to install. Tested on H11SSL; it should work on every board
that accepts the X10/X11/X12/H11/H12 fan commands (`raw 0x30 0x70 0x66`).

## What it does

- Reads the CPU temperature (hwmon: `k10temp`, `zenpower`, `coretemp`), every disk through SMART
  (`smartctl --json`, without waking sleeping disks) and the BMC sensors.
- Drives each IPMI zone (0 = `FAN1…`, 1 = `FANA…`) from one or more sources, each with its own curve.
  The zone runs at the highest speed its sources ask for, with hysteresis and min/max limits.
- If a source cannot be read, the zone goes to the failsafe speed (100% by default). When the service
  stops or crashes, fans are handed back to the BMC.
- Web UI: live overview, curve editor, history (SQLite), manual speed, alerts, settings, journal.
- Alerts (hot CPU or disk, stopped fan, unreachable BMC) sent to Discord, ntfy, Gotify or any webhook.
- Prometheus metrics on `/metrics`.
- Local BMC (`/dev/ipmi0`) or remote BMC over the network (IPMI over LAN).

## Install

Requirements on the server: `ipmitool`, `smartmontools`, `lsblk` (already present on TrueNAS SCALE).

```bash
# Debian / Proxmox
sudo apt install ipmitool smartmontools
curl -fsSLO https://raw.githubusercontent.com/SeiyaGame/fan_control_supermicro/main/install.sh
sudo bash install.sh

# TrueNAS SCALE: install on a pool, the boot pool is replaced on updates
sudo bash install.sh --dir /mnt/tank/apps/fan-control
```

Then open `http://<server>:9495` and choose the web password. The configuration is in `config.toml`
next to the binary; everything can be changed from the UI. `config.example.toml` is a complete example
(curves tuned to keep hard drives at or below ~40°C). After editing the file by hand, use *Reload the
configuration file* in Settings: no restart needed.

On TrueNAS, add the command printed by the installer as a *Post Init* script (System › Advanced ›
Init/Shutdown Scripts) so the service is recreated after a TrueNAS update.

### Docker

```bash
docker compose up -d --build
```

| Variable | Default | |
| -------- | ------- | - |
| `PUID` / `PGID` | `1000` | Owner of the files in `/data` (config, history) |
| `RUN_AS_ROOT` | `true` | The process needs root for `/dev/ipmi0` and SMART; files still belong to `PUID:PGID`. Set `false` to run as `PUID:PGID`, only with a remote BMC and disk monitoring disabled |
| `TZ` | | Time zone of the logs |

The container must be privileged to reach `/dev/ipmi0` and the disks. With a remote BMC and no disk
monitoring, it can run unprivileged with `RUN_AS_ROOT=false`.

## Command line

```
fan-control [--config FILE] [--dry-run] [--simulate] [--listen ADDR]
fan-control set-password      # reset the web password (read from stdin)
fan-control check-config      # validate the configuration
```

- `--dry-run`: read and compute, never write to the BMC.
- `--simulate`: fake BMC and sensors, to try the UI on any Linux machine.

Logs go to the journal (`journalctl -u fan-control -f`); the last 1000 events are also in the UI.

## Development

| Tool | Version |
| ---- | ------- |
| Rust | stable (≥ 1.88), target `x86_64-unknown-linux-musl` for releases |
| Node | 24 |

```bash
# Backend with fake hardware
cargo run -- --simulate --config ./dev-config.toml --listen 127.0.0.1:9495

# UI with hot reload (proxies the API to the backend above)
cd web && npm ci && npm run dev

# Static release binary (UI embedded)
cd web && npm run build && cd .. && cargo build --release --target x86_64-unknown-linux-musl
```

Quality checks run before each commit and in CI:

```bash
git config core.hooksPath .githooks   # once per clone
scripts/check.sh                      # fmt, clippy, tests, eslint, prettier, svelte-check, vitest
```

On Windows, the hook runs the Rust checks inside WSL when `cargo` is not on the Windows `PATH`.

### Layout

```
src/
  main.rs         command line, startup, shutdown and panic handling
  controller.rs   control loop: read sensors, compute duties, apply them
  curve.rs        curves and hysteresis
  ipmi.rs         ipmitool commands and `sdr` parsing
  sensors/        hwmon, disks (lsblk + smartctl), temperature sources
  alerts.rs       alert detection;  notify.rs  notification channels
  history.rs      SQLite time series
  web/            HTTP API, authentication, Prometheus metrics, embedded UI
  sim.rs          simulated hardware
web/              Svelte 5 + TypeScript UI
tests/api.rs      HTTP API tests
```

Releases: push a `v*` tag, CI builds the static binary and attaches it to the GitHub release.

## License

GPL-3.0
