import { describe, expect, it } from 'vitest'
import { curveProblems, dutyAt, insertPoint } from './curve'
import { band, probeForSource } from './thermal'

describe('dutyAt', () => {
  const points: [number, number][] = [
    [30, 30],
    [40, 50],
    [60, 100],
  ]

  it('matches the backend interpolation', () => {
    expect(dutyAt(points, 10)).toBe(30)
    expect(dutyAt(points, 35)).toBe(40)
    expect(dutyAt(points, 50)).toBe(75)
    expect(dutyAt(points, 90)).toBe(100)
  })

  it('runs at full speed without points', () => {
    expect(dutyAt([], 40)).toBe(100)
  })
})

describe('curveProblems', () => {
  it('accepts a valid curve', () => {
    expect(
      curveProblems([
        [30, 20],
        [50, 100],
      ]),
    ).toEqual([])
  })

  it('rejects unsorted temperatures and bad duties', () => {
    expect(
      curveProblems([
        [50, 20],
        [40, 30],
      ]),
    ).toHaveLength(1)
    expect(curveProblems([[40, 120]])).toHaveLength(1)
    expect(curveProblems([])).toHaveLength(1)
  })
})

describe('insertPoint', () => {
  it('keeps points sorted and replaces same temperature', () => {
    expect(
      insertPoint(
        [
          [30, 20],
          [50, 80],
        ],
        [40, 50],
      ),
    ).toEqual([
      [30, 20],
      [40, 50],
      [50, 80],
    ])
    expect(insertPoint([[30, 20]], [30, 40])).toEqual([[30, 40]])
  })
})

describe('thermal bands', () => {
  it('depends on what is measured', () => {
    expect(band(45, 'hdd')).toBe('hot')
    expect(band(45, 'cpu')).toBe('mild')
    expect(band(null, 'cpu')).toBeNull()
    expect(band(99, 'nvme')).toBe('critical')
  })

  it('guesses the probe from a source id', () => {
    expect(probeForSource('cpu')).toBe('cpu')
    expect(probeForSource('disks:nvme')).toBe('nvme')
    expect(probeForSource('disk:ABC', () => 'ssd')).toBe('ssd')
    expect(probeForSource('ipmi:System Temp')).toBe('board')
  })
})
