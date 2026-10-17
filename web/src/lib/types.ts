// Mirrors the Rust structures serialized by the API (src/config.rs, src/state.rs).

export type FanMode = 'standard' | 'full' | 'optimal' | 'heavy_io'
export type DiskKind = 'hdd' | 'ssd' | 'nvme'
export type DiskState = 'active' | 'standby' | 'error'
export type ZoneMode = 'auto' | 'manual' | 'failsafe' | 'unmanaged'
export type Severity = 'info' | 'warning' | 'critical'

export interface Disk {
  name: string
  serial: string
  model: string
  kind: DiskKind
  state: DiskState
  temperature: number | null
  error: string | null
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
    repeat_minutes: number
    notify_resolved: boolean
  }
  notifications: NotificationChannel[]
  zones: Zone[]
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
