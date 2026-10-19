import { describe, expect, it } from 'vitest'
import { healthLevel, powerOnText } from './health'
import type { Disk } from './types'

const disk = (health: Disk['health']): Disk => ({
  name: 'sda',
  serial: 'S1',
  model: '',
  kind: 'hdd',
  state: 'active',
  temperature: 35,
  error: null,
  health,
})

describe('healthLevel', () => {
  it('follows the status computed by the server', () => {
    const health = {
      status: 'warning' as const,
      passed: true,
      power_on_hours: 10,
      reallocated: 8,
      pending: 0,
      crc_errors: 0,
      media_errors: null,
      percentage_used: null,
    }
    expect(healthLevel(disk(health))).toBe('warning')
  })

  it('is unknown without SMART data', () => {
    expect(healthLevel(disk(null))).toBe('unknown')
  })
})

describe('powerOnText', () => {
  it('reads in years and days', () => {
    expect(powerOnText(31200)).toBe('3 years 205 days')
    expect(powerOnText(24 * 366)).toBe('1 year 1 day')
    expect(powerOnText(48)).toBe('2 days')
    expect(powerOnText(5)).toBe('5 hours')
  })
})
