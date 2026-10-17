import type { CurvePoint } from './types'

/** Same interpolation as `Curve::duty_at` in src/curve.rs. */
export function dutyAt(points: CurvePoint[], temp: number): number {
  if (points.length === 0) return 100
  if (temp <= points[0][0]) return points[0][1]
  for (let i = 1; i < points.length; i++) {
    const [t0, d0] = points[i - 1]
    const [t1, d1] = points[i]
    if (temp <= t1) return d0 + ((temp - t0) / (t1 - t0)) * (d1 - d0)
  }
  return points[points.length - 1][1]
}

/** Problems that the backend would reject, in plain words. */
export function curveProblems(points: CurvePoint[]): string[] {
  const problems: string[] = []
  if (points.length === 0) problems.push('Add at least one point.')
  for (let i = 1; i < points.length; i++) {
    if (points[i][0] <= points[i - 1][0]) {
      problems.push(`Point ${i + 1} must be hotter than point ${i}.`)
    }
  }
  for (const [t, d] of points) {
    if (d < 0 || d > 100) problems.push(`Speed ${d}% is outside 0–100%.`)
    if (t < -20 || t > 150) problems.push(`Temperature ${t}°C is outside −20–150°C.`)
  }
  return problems
}

/** Inserts a point keeping temperatures sorted. */
export function insertPoint(points: CurvePoint[], point: CurvePoint): CurvePoint[] {
  const next = points.filter(([t]) => t !== point[0])
  next.push(point)
  return next.sort((a, b) => a[0] - b[0])
}
