#!/bin/sh
# Container entrypoint: maps the abc user to PUID/PGID so that /data belongs to the host user.
set -e

PUID=${PUID:-1000}
PGID=${PGID:-1000}
RUN_AS_ROOT=${RUN_AS_ROOT:-false}

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
    # Everything as root (troubleshooting). Files keep the PUID/PGID owner.
    exec fan-control "$@"
fi
# fan-control runs as abc (PUID/PGID). ipmitool and smartctl are wrappers that run the real
# tools as root through doas, the only commands abc is allowed to run that way.
exec su-exec abc fan-control "$@"
