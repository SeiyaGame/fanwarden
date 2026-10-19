// Mirrors the Rust structures serialized by the API (src/config.rs, src/state.rs).

export type FanMode = 'standard' | 'full' | 'optimal' | 'heavy_io'
export type DiskKind = 'hdd' | 'ssd' | 'nvme'
export type DiskState = 'active' | 'standby' | 'error'
export type ZoneMode = 'auto' | 'manual' | 'failsafe' | 'unmanaged' | 'calibrating'
export type Severity = 'info' | 'warning' | 'critical'
export type HealthStatus = 'ok' | 'warning' | 'failing'

/** SMART counters; null when the drive does not report them. */
export interface DiskHealth {
  status: HealthStatus
  passed: boolean | null
  power_on_hours: number | null
  reallocated: number | null
  pending: number | null
  crc_errors: number | null
  media_errors: number | null
  percentage_used: number | null
}

export interface Disk {
  name: string
  serial: string
  model: string
  kind: DiskKind
  state: DiskState
  temperature: number | null
  error: string | null
  health: DiskHealth | null
}

export interface FanReading {
  name: string
  rpm: number | null
  status: string
}

export interface TempReading {
  name: string
  celsius: number | null
}

export interface RuleStatus {
  source: string
  temperature: number | null
  state: 'ok' | 'idle' | 'missing'
  duty: number
  target: number | null
  emergency: boolean
}

export interface ZoneStatus {
  id: number
  name: string
  mode: ZoneMode
  duty: number | null
  driver: string | null
  rules: RuleStatus[]
}

export interface SourceOption {
  id: string
  label: string
  celsius: number | null
}

export interface FanCurve {
  name: string
  /** [duty, rpm] from 100% down; 0 RPM when the fan stopped. */
  points: [number, number][]
  stall_duty: number | null
}

export interface CalibrationReference {
  zone: number
  ts: number
  fans: FanCurve[]
  stall_duty: number | null
  suggested_min_duty: number | null
  bmc_takeover: number | null
}

export type CalibrationStatus =
  | { state: 'running'; zone: number; duty: number; step: number; steps: number }
  | { state: 'done'; zone: number }
  | { state: 'failed'; zone: number; reason: string }

export interface ActiveAlert {
  key: string
  message: string
  severity: Severity
  since: number
}

export interface Snapshot {
  ts: number
  version: string
  simulate: boolean
  dry_run: boolean
  fan_mode: FanMode | null
  bmc_error: string | null
  sensors: {
    cpu: number | null
    hwmon: { chip: string; label: string; celsius: number }[]
    disks: Disk[]
    disks_fresh: boolean
    ipmi: TempReading[]
  }
  fans: FanReading[]
  zones: ZoneStatus[]
  sources: SourceOption[]
  alerts: ActiveAlert[]
  calibration: CalibrationStatus | null
  references: Record<string, CalibrationReference>
}

export type CurvePoint = [number, number]

export type Reactivity = 'calm' | 'normal' | 'reactive'

export interface Target {
  celsius: number
  reactivity: Reactivity
}

export interface Rule {
  source: string
  curve: CurvePoint[]
  target?: Target | null
}

export interface Zone {
  id: number
  name: string
  hysteresis: number
  min_duty: number
  max_duty: number
  rules: Rule[]
}

export type NotificationChannel =
  | { type: 'discord'; enabled: boolean; url: string }
  | { type: 'ntfy'; enabled: boolean; server: string; topic: string; token: string }
  | { type: 'gotify'; enabled: boolean; server: string; token: string }
  | { type: 'webhook'; enabled: boolean; url: string }

export interface Config {
  general: {
    interval_secs: number
    dry_run: boolean
    failsafe_duty: number
    reapply_secs: number
    exit_fan_mode: FanMode
  }
  ipmi: {
    target: 'local' | 'remote'
    ipmitool_path: string
    timeout_secs: number
    host: string
    username: string
    password: string
    interface: 'lan' | 'lanplus'
  }
  disks: {
    enabled: boolean
    smartctl_path: string
    lsblk_path: string
    interval_secs: number
    exclude: string
    skip_standby: boolean
  }
  web: { listen: string; password_hash: string; session_hours: number; metrics_enabled: boolean }
  history: { enabled: boolean; retention_days: number }
  regulation: { safety_margin_disks: number; safety_margin_cpu: number }
  alerts: {
    cpu_high: number | null
    disk_high: number | null
    nvme_high: number | null
    disk_low: number | null
    fan_min_rpm: number | null
    ipmi_failures: number
    fan_slowdown_percent: number | null
    smart_health: boolean
    repeat_minutes: number
    notify_resolved: boolean
  }
  notifications: NotificationChannel[]
  zones: Zone[]
}

export interface ConfigVersion {
  id: number
  ts: number
  origin: { kind: 'startup' | 'ui' | 'file' } | { kind: 'restore'; from_ts: number }
  /** TOML, secrets replaced by `(secret n)`. */
  toml: string
}

export interface LogEvent {
  id: number
  ts: number
  level: 'info' | 'warn' | 'error'
  message: string
}

export interface HistoryData {
  timestamps: number[]
  series: { key: string; values: (number | null)[] }[]
}
