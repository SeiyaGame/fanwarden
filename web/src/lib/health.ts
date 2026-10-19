// SMART health of a disk, as shown on its bay.

import type { Disk, HealthStatus } from './types'

export type HealthLevel = HealthStatus | 'unknown'

export function healthLevel(disk: Disk): HealthLevel {
  return disk.health?.status ?? 'unknown'
}

export const HEALTH_TEXT: Record<HealthLevel, string> = {
  ok: 'Healthy',
  warning: 'Worn or damaged media: keep an eye on it and a backup at hand',
  failing: 'SMART reports the drive as failing: replace it',
  unknown: 'No SMART health reported yet',
}

/** 31200 → "3 years 205 days". */
export function powerOnText(hours: number): string {
  const days = Math.floor(hours / 24)
  const years = Math.floor(days / 365)
  const rest = days % 365
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
  if (years > 0) return `${plural(years, 'year')} ${plural(rest, 'day')}`
  if (days > 0) return plural(days, 'day')
  return plural(hours, 'hour')
}
