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
RUN apk add --no-cache ipmitool smartmontools lsblk tzdata
COPY --from=build /src/target/release/fan-control /usr/local/bin/fan-control
ENV FAN_CONTROL_CONFIG=/data/config.toml
VOLUME /data
EXPOSE 9495
ENTRYPOINT ["fan-control"]
