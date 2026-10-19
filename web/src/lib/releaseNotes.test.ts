import { describe, expect, it } from 'vitest'
import { parseNotes } from './releaseNotes'

describe('parseNotes', () => {
  it('reads git-cliff sections without links nor commit references', () => {
    const notes = `### Features

- Configuration history with diffs and restore (#5) ([6c4c692](https://github.com/x/y/commit/6c4c692d74f2))

### Bug fixes

- **calibration:** Suggest a minimum speed ([a2610b9](https://github.com/x/y/commit/a2610b92d8a7))
- See [the docs](https://example.com) for \`install.sh\`
`
    expect(parseNotes(notes)).toEqual([
      { title: 'Features', items: ['Configuration history with diffs and restore (#5)'] },
      {
        title: 'Bug fixes',
        items: ['calibration: Suggest a minimum speed', 'See the docs for install.sh'],
      },
    ])
  })

  it('keeps bullets without a heading and drops empty sections', () => {
    expect(parseNotes('Intro\n\n## Empty\n\n* one\n')).toEqual([{ title: 'Empty', items: ['one'] }])
    expect(parseNotes('- alone')).toEqual([{ title: '', items: ['alone'] }])
  })
})
