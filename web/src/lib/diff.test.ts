import { describe, expect, it } from 'vitest'
import { countChanges, diffLines, withContext } from './diff'

describe('diffLines', () => {
  it('marks a changed line as removed then added', () => {
    expect(diffLines('a\nb\nc', 'a\nB\nc')).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'del', text: 'b' },
      { kind: 'add', text: 'B' },
      { kind: 'same', text: 'c' },
    ])
  })

  it('handles lines added at the end and removed at the start', () => {
    const lines = diffLines('x\na\nb', 'a\nb\nc\nd')
    expect(lines.map((l) => l.kind)).toEqual(['del', 'same', 'same', 'add', 'add'])
    expect(countChanges(lines)).toEqual({ added: 2, removed: 1 })
  })

  it('finds no change in identical texts', () => {
    expect(countChanges(diffLines('a\nb', 'a\nb'))).toEqual({ added: 0, removed: 0 })
  })
})

describe('withContext', () => {
  it('folds unchanged lines far from a change', () => {
    const before = ['1', '2', '3', '4', '5', '6', '7', '8', '9'].join('\n')
    const after = before.replace('5', 'five')
    const rows = withContext(diffLines(before, after), 1)
    expect(rows).toEqual([
      { kind: 'fold', count: 3 },
      { kind: 'same', text: '4' },
      { kind: 'del', text: '5' },
      { kind: 'add', text: 'five' },
      { kind: 'same', text: '6' },
      { kind: 'fold', count: 3 },
    ])
  })
})
