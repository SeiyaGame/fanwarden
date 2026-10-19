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

# Runtime: the tools fanwarden calls
FROM alpine:3.24
RUN apk add --no-cache ipmitool smartmontools lsblk tzdata shadow su-exec libcap-setcap \
    && addgroup -g 1000 abc \
    && adduser -D -H -u 1000 -G abc abc
COPY --from=build /src/target/release/fanwarden /usr/local/bin/fanwarden
COPY --chmod=755 docker/entrypoint.sh /entrypoint.sh
ENV FANWARDEN_CONFIG=/data/config.toml \
    PUID=1000 \
    PGID=1000 \
    RUN_AS_ROOT=true
VOLUME /data
EXPOSE 9495
HEALTHCHECK --interval=30s --timeout=10s --start-period=20s --retries=3 CMD ["fanwarden", "healthcheck"]
ENTRYPOINT ["/entrypoint.sh"]
