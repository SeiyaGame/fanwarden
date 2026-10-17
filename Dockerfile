# Web UI
FROM node:24-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Static binary (the UI is embedded at compile time)
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY --from=web /web/dist ./web/dist
RUN cargo build --release --locked

# Runtime: the tools fan-control calls
FROM alpine:3.22
# fan-control runs as abc; only ipmitool and smartctl (/dev/ipmi0 and SMART need root) run as
# root, through doas. Wrappers with the same names come first in PATH, so the app needs no change.
RUN apk add --no-cache ipmitool smartmontools lsblk tzdata shadow su-exec doas \
    && addgroup -g 1000 abc \
    && adduser -D -H -u 1000 -G abc abc \
    && echo "permit nopass keepenv root" > /etc/doas.d/fan-control.conf \
    && for tool in ipmitool smartctl; do \
         real="$(command -v "$tool")"; \
         printf '#!/bin/sh\nexec doas %s "$@"\n' "$real" > "/usr/local/bin/$tool"; \
         chmod 755 "/usr/local/bin/$tool"; \
         echo "permit nopass setenv { IPMI_PASSWORD } abc as root cmd $real" >> /etc/doas.d/fan-control.conf; \
       done \
    && chmod 400 /etc/doas.d/fan-control.conf
COPY --from=build /src/target/release/fan-control /usr/local/bin/fan-control
COPY --chmod=755 docker/entrypoint.sh /entrypoint.sh
ENV FAN_CONTROL_CONFIG=/data/config.toml \
    PUID=1000 \
    PGID=1000 \
    RUN_AS_ROOT=false
VOLUME /data
EXPOSE 9495
ENTRYPOINT ["/entrypoint.sh"]
