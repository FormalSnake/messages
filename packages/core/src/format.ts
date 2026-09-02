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

export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean)
  if (words.length === 0) return '#'
  if (/^[+\d(]/.test(name)) return '#'
  const first = words[0]?.[0] ?? ''
  const last = words.length > 1 ? (words[words.length - 1]?.[0] ?? '') : ''
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
