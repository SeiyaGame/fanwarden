/** Release notes written by git-cliff: `### Section` headings and `- item` bullets. */
export interface NotesSection {
  title: string
  items: string[]
}

/** Markdown links, commit references and emphasis removed: the notes are shown as plain text. */
function plain(text: string): string {
  return text
    .replace(/\s*\(\[[0-9a-f]{7,}\]\([^)]*\)\)/g, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\*\*([^*]*)\*\*/g, '$1')
    .replace(/`([^`]*)`/g, '$1')
    .trim()
}

export function parseNotes(markdown: string): NotesSection[] {
  const sections: NotesSection[] = []
  for (const line of markdown.split(/\r?\n/)) {
    const heading = line.match(/^#{1,4}\s+(.*)/)
    if (heading) {
      sections.push({ title: plain(heading[1]), items: [] })
    } else if (/^\s*[-*]\s+/.test(line)) {
      if (!sections.length) sections.push({ title: '', items: [] })
      sections.at(-1)!.items.push(plain(line.replace(/^\s*[-*]\s+/, '')))
    }
  }
  return sections.filter((s) => s.items.length)
}
