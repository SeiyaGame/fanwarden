// Thermal scale: maps a reading to one of five colors, with bands that depend on what is measured
// (a disk at 45°C is hot, a CPU at 45°C is idle).

export type Band = 'cool' | 'mild' | 'warm' | 'hot' | 'critical'
export type Probe = 'cpu' | 'hdd' | 'ssd' | 'nvme' | 'board'

const BANDS: Record<Probe, [number, number, number, number]> = {
  // Upper bound of cool, mild, warm, hot; above the last one is critical.
  cpu: [45, 60, 72, 85],
  hdd: [32, 38, 43, 48],
  ssd: [40, 50, 60, 70],
  nvme: [45, 55, 65, 75],
  board: [35, 45, 55, 65],
}

const ORDER: Band[] = ['cool', 'mild', 'warm', 'hot', 'critical']

export function band(celsius: number | null | undefined, probe: Probe): Band | null {
  if (celsius === null || celsius === undefined || Number.isNaN(celsius)) return null
  const limits = BANDS[probe]
  const index = limits.findIndex((limit) => celsius < limit)
  return ORDER[index === -1 ? 4 : index]
}

export function thermalColor(celsius: number | null | undefined, probe: Probe): string {
  const b = band(celsius, probe)
  return b ? `var(--${b})` : 'var(--rule)'
}

/** Probe type for a source id such as `cpu`, `disks:hdd` or `disk:SERIAL`. */
export function probeForSource(source: string, diskKind?: (serial: string) => Probe | undefined): Probe {
  if (source === 'cpu' || source.startsWith('hwmon:k10temp') || source.startsWith('hwmon:coretemp'))
    return 'cpu'
  if (source === 'disks:hdd' || source === 'disks:all') return 'hdd'
  if (source === 'disks:ssd') return 'ssd'
  if (source === 'disks:nvme' || source.startsWith('hwmon:nvme')) return 'nvme'
  if (source.startsWith('disk:')) return diskKind?.(source.slice(5)) ?? 'hdd'
  if (source.toLowerCase().includes('cpu')) return 'cpu'
  return 'board'
}
