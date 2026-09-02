/** Hand a URL or file to the desktop: xdg-open on Linux, open on macOS. */
export function openExternal(target: string): void {
  const command = process.platform === 'darwin' ? ['open', target] : ['xdg-open', target]
  try {
    Bun.spawn(command, { stdout: 'ignore', stderr: 'ignore' })
  } catch (error) {
    console.error(`open: ${String(error)}`)
  }
}

const URL_PATTERN = /https?:\/\/[^\s<>"'）)]+|www\.[^\s<>"'）)]+/gi

export type TextSegment = { kind: 'text'; value: string } | { kind: 'link'; value: string; href: string }

export function splitLinks(text: string): TextSegment[] {
  const segments: TextSegment[] = []
  let last = 0
  for (const match of text.matchAll(URL_PATTERN)) {
    const start = match.index ?? 0
    const raw = match[0]
    const trimmed = raw.replace(/[.,;:!?]+$/, '')
    if (start > last) segments.push({ kind: 'text', value: text.slice(last, start) })
    segments.push({ kind: 'link', value: trimmed, href: trimmed.startsWith('http') ? trimmed : `https://${trimmed}` })
    last = start + trimmed.length
  }
  if (last < text.length) segments.push({ kind: 'text', value: text.slice(last) })
  return segments
}
