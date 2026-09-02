const DAY = 24 * 60 * 60 * 1000

function startOfDay(ms: number): number {
  const date = new Date(ms)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

export function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

function weekday(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { weekday: 'long' })
}

function shortDate(ms: number, now: number): string {
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear()
  return new Date(ms).toLocaleDateString(undefined, sameYear ? { month: 'short', day: 'numeric' } : { year: 'numeric', month: 'short', day: 'numeric' })
}

/** Sidebar timestamp: time today, "Yesterday", a weekday inside the week, else a date. */
export function formatListDate(ms: number, now = Date.now()): string {
  const today = startOfDay(now)
  const day = startOfDay(ms)
  if (day === today) return formatTime(ms)
  if (day === today - DAY) return 'Yesterday'
  if (day > today - 6 * DAY) return weekday(ms)
  return shortDate(ms, now)
}

/** Separator above a run of messages: "Today 9:41 AM", "Yesterday 6:02 PM", "Monday 8:15 AM", "Sep 2, 2025 at 4:30 PM". */
export function formatSeparator(ms: number, now = Date.now()): string {
  const today = startOfDay(now)
  const day = startOfDay(ms)
  const time = formatTime(ms)
  if (day === today) return `Today ${time}`
  if (day === today - DAY) return `Yesterday ${time}`
  if (day > today - 6 * DAY) return `${weekday(ms)} ${time}`
  return `${shortDate(ms, now)} at ${time}`
}

/** Messages.app inserts a separator when more than an hour passed since the previous message. */
export function needsSeparator(previous: number | undefined, current: number): boolean {
  return previous === undefined || current - previous > 60 * 60 * 1000
}

function hhmm(ms: number): string {
  const date = new Date(ms)
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

/** A location's staleness: "just now", "5 min ago", "2 h ago", then a day-relative "Yesterday 18:02". */
export function relativeTime(ms: number, now = Date.now()): string {
  const diff = now - ms
  if (diff < 60_000) return 'just now'
  if (diff < 60 * 60_000) return `${Math.floor(diff / 60_000)} min ago`
  if (diff < DAY) return `${Math.floor(diff / (60 * 60_000))} h ago`
  const today = startOfDay(now)
  const day = startOfDay(ms)
  if (day === today - DAY) return `Yesterday ${hhmm(ms)}`
  if (day > today - 6 * DAY) return `${weekday(ms)} ${hhmm(ms)}`
  return `${shortDate(ms, now)} at ${hhmm(ms)}`
}

export function formatAddress(address: string): string {
  if (address.includes('@')) return address
  const digits = address.replace(/[^\d+]/g, '')
  const match = /^\+1(\d{3})(\d{3})(\d{4})$/.exec(digits)
  if (match) return `+1 (${match[1]}) ${match[2]}-${match[3]}`
  if (digits.startsWith('+')) {
    const body = digits.slice(1)
    const country = body.length > 9 ? body.slice(0, body.length - 9) : ''
    const rest = body.slice(country.length)
    const groups = rest.match(/.{1,3}/g) ?? [rest]
    return `+${country} ${groups.join(' ')}`.trim()
  }
  return address
}

const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

/** First user-perceived character. Indexing a string would split an emoji into a lone surrogate, which the native JSON parser rejects. */
export function firstGrapheme(value: string): string {
  const first = graphemes.segment(value)[Symbol.iterator]().next()
  return first.done ? '' : first.value.segment
}

export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean)
  if (words.length === 0) return '#'
  if (/^[+\d(]/.test(name)) return '#'
  const first = firstGrapheme(words[0] ?? '')
  const last = words.length > 1 ? firstGrapheme(words[words.length - 1] ?? '') : ''
  return (first + last).toUpperCase()
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`
}
