#!/bin/sh
# Container entrypoint.
#   RUN_AS_ROOT=true  (default) everything runs as root, in a privileged container with /dev.
#   RUN_AS_ROOT=false hardened mode: fan-control runs as PUID/PGID, the container only gets
#                     the devices it needs (see "Hardened mode" in the README).
set -e

PUID=${PUID:-1000}
PGID=${PGID:-1000}
RUN_AS_ROOT=${RUN_AS_ROOT:-true}

# Adjust abc user/group to match requested PUID/PGID
if [ "$(id -g abc)" != "$PGID" ]; then
    groupmod -o -g "$PGID" abc
fi
if [ "$(id -u abc)" != "$PUID" ]; then
    usermod -o -u "$PUID" abc
fi

echo "
──────────────────────────────────
GID/UID
──────────────────────────────────
User uid:    $PUID
User gid:    $PGID
Run as root: $RUN_AS_ROOT
──────────────────────────────────"

# Create the default configuration on first start, then hand /data to the host user.
mkdir -p "$(dirname "$FAN_CONTROL_CONFIG")"
[ -f "$FAN_CONTROL_CONFIG" ] || fan-control check-config >/dev/null
chown -R abc:abc "$(dirname "$FAN_CONTROL_CONFIG")"

if [ "$RUN_AS_ROOT" = "true" ]; then
    # Files keep the PUID/PGID owner: fan-control preserves it when saving the configuration.
    exec fan-control "$@"
fi

# ---- Hardened mode -------------------------------------------------------------------------

# The device nodes below are changed. They must be the container's own copies (`devices:`),
# never the host's /dev mounted as a volume.
dev_fs="$(awk '$5 == "/dev" { for (i = 7; i <= NF; i++) if ($i == "-") print $(i + 1) }' /proc/self/mountinfo | tail -n1)"
if [ "$dev_fs" = "devtmpfs" ]; then
    echo "RUN_AS_ROOT=false: remove the /dev volume and list the devices under 'devices:' instead" >&2
    exit 1
fi

# Give abc access to the BMC and the disks passed to the container.
found=""
for dev in /dev/ipmi* /dev/sd* /dev/nvme*; do
    [ -b "$dev" ] || [ -c "$dev" ] || continue
    chgrp abc "$dev"
    chmod g+rw "$dev"
    found="$found ${dev#/dev/}"
done
echo "Devices for abc:${found:- none}"

# SMART needs CAP_SYS_RAWIO (SATA/SAS) and CAP_SYS_ADMIN (NVMe). Grant smartctl the ones the
# container allows (cap_add); a capability missing from the bounding set would make it unusable.
bounding="0x$(awk '/^CapBnd/ { print $2 }' /proc/self/status)"
caps=""
[ $(( (bounding >> 17) & 1 )) -eq 1 ] && caps="cap_sys_rawio"
[ $(( (bounding >> 21) & 1 )) -eq 1 ] && caps="${caps:+$caps,}cap_sys_admin"
if [ -n "$caps" ]; then
    setcap "$caps+ep" "$(command -v smartctl)"
    echo "smartctl capabilities: $caps"
else
    echo "warning: no SYS_RAWIO capability, disk temperatures will not be readable (cap_add: SYS_RAWIO)" >&2
fi

exec su-exec abc fan-control "$@"
