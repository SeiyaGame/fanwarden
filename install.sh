#!/usr/bin/env bash
# Installs fan-control as a systemd service (Debian, Proxmox, TrueNAS SCALE...).
#
#   sudo ./install.sh                         download the latest release into /opt/fan-control
#   sudo ./install.sh --binary ./fan-control  install a binary you built yourself
#   sudo ./install.sh --dir /mnt/tank/apps/fan-control
#   sudo ./install.sh --service-only --dir DIR   only (re)create the service (TrueNAS post-init)
set -euo pipefail

REPO="${FAN_CONTROL_REPO:-SeiyaGame/fan_control_supermicro}"
DIR=/opt/fan-control
BINARY=""
SERVICE_ONLY=false
UNIT=/etc/systemd/system/fan-control.service

while [ $# -gt 0 ]; do
  case "$1" in
    --dir) DIR="$2"; shift 2 ;;
    --binary) BINARY="$2"; shift 2 ;;
    --service-only) SERVICE_ONLY=true; shift ;;
    -h | --help) sed -n '2,8p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)" >&2; exit 1; }

is_truenas=false
if [ -e /etc/truenas_version ] || command -v midclt >/dev/null 2>&1; then
  is_truenas=true
  # The TrueNAS boot pool is replaced on every update: keep everything on a data pool.
  case "$DIR" in
    /mnt/*) ;;
    *) echo "On TrueNAS, install on a pool: --dir /mnt/<pool>/apps/fan-control" >&2; exit 1 ;;
  esac
fi

for tool in ipmitool smartctl lsblk; do
  command -v "$tool" >/dev/null 2>&1 || echo "warning: $tool not found (apt install ipmitool smartmontools util-linux)" >&2
done

mkdir -p "$DIR"
BIN="$DIR/fan-control"
CONFIG="$DIR/config.toml"

if ! $SERVICE_ONLY; then
  if [ -n "$BINARY" ]; then
    install -m 0755 "$BINARY" "$BIN"
  else
    url="https://github.com/$REPO/releases/latest/download/fan-control-x86_64-linux"
    echo "Downloading $url"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -fsSL "$url" -o "$tmp/fan-control-x86_64-linux"
    curl -fsSL "$url.sha256" -o "$tmp/fan-control-x86_64-linux.sha256"
    (cd "$tmp" && sha256sum -c fan-control-x86_64-linux.sha256)
    install -m 0755 "$tmp/fan-control-x86_64-linux" "$BIN"
  fi
  "$BIN" --config "$CONFIG" check-config
  chmod 600 "$CONFIG"
fi

[ -x "$BIN" ] || { echo "$BIN not found: install it first" >&2; exit 1; }

cat >"$UNIT" <<EOF
[Unit]
Description=Supermicro fan control
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$BIN --config $CONFIG
Restart=always
RestartSec=5
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable fan-control >/dev/null 2>&1 || true
systemctl restart fan-control

port="$(sed -n 's/^listen = ".*:\([0-9]*\)"/\1/p' "$CONFIG" | head -n1)"
echo
echo "fan-control is running: http://$(hostname -I 2>/dev/null | awk '{print $1}'):${port:-9495}"
echo "Open it to choose the web password. Logs: journalctl -u fan-control -f"
if $is_truenas; then
  echo
  echo "TrueNAS resets /etc on updates. Add this under System > Advanced > Init/Shutdown Scripts"
  echo "(type Command, when Post Init) so the service comes back after an update:"
  echo "  $DIR/install.sh --service-only --dir $DIR"
  cp "$0" "$DIR/install.sh" 2>/dev/null && chmod 755 "$DIR/install.sh" || true
fi
