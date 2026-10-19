// Line diff of two texts (longest common subsequence), shown like a git diff: changed lines with
// a little context, unchanged stretches folded.

export type DiffLine = { kind: 'same' | 'add' | 'del'; text: string }
export type DiffRow = DiffLine | { kind: 'fold'; count: number }

export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split('\n')
  const b = after.split('\n')
  // lcs[i][j]: length of the common subsequence of a[i..] and b[j..]
  const lcs = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0))
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1])
    }
  }
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      out.push({ kind: 'same', text: a[i] })
      i++
      j++
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ kind: 'del', text: a[i++] })
    } else {
      out.push({ kind: 'add', text: b[j++] })
    }
  }
  while (i < a.length) out.push({ kind: 'del', text: a[i++] })
  while (j < b.length) out.push({ kind: 'add', text: b[j++] })
  return out
}

/** Keeps `context` unchanged lines around each change and folds the rest. */
export function withContext(lines: DiffLine[], context = 2): DiffRow[] {
  const near = lines.map((_, index) =>
    lines.slice(Math.max(0, index - context), index + context + 1).some((line) => line.kind !== 'same'),
  )
  const rows: DiffRow[] = []
  let folded = 0
  lines.forEach((line, index) => {
    if (near[index]) {
      if (folded) rows.push({ kind: 'fold', count: folded })
      folded = 0
      rows.push(line)
    } else {
      folded++
    }
  })
  if (folded) rows.push({ kind: 'fold', count: folded })
  return rows
}

export function countChanges(lines: DiffLine[]): { added: number; removed: number } {
  return {
    added: lines.filter((l) => l.kind === 'add').length,
    removed: lines.filter((l) => l.kind === 'del').length,
  }
}
