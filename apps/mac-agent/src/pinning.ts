/**
 * Reads the pinned conversations Messages.app keeps in
 * `~/Library/Preferences/com.apple.messages.pinning.plist` so a pin made in
 * the real app is visible to every Linux client, not just pins made here.
 *
 * `defaults export` goes through cfprefsd, so it reflects a pin made moments
 * ago even if Messages.app hasn't flushed the file to disk yet. `plutil
 * -extract ... json` is then run against that XML over stdin rather than
 * the file directly, for the same reason. `pD.pT` (the last-edit date) is
 * extracted separately with `-extract ... raw`: plutil's JSON output
 * has no date type and errors on the whole plist if a date is included.
 */

const DOMAIN = 'com.apple.messages.pinning'

interface PinGroupEntry {
  o?: string
  h?: string
}

export interface MacPins {
  identifiers: string[]
  updatedAt: number | null
}

let warned = false

function warnOnce(message: string): void {
  if (warned) return
  warned = true
  console.error(`pinning: ${message}`)
}

function exportDomain(domain: string): Uint8Array | null {
  const result = Bun.spawnSync(['defaults', 'export', domain, '-'])
  return result.success && result.stdout.length > 0 ? result.stdout : null
}

/** `-o - -` means "write to stdout, read the plist from stdin". */
function extractKeypath(xml: Uint8Array, keypath: string, format: 'json' | 'raw'): string | null {
  const result = Bun.spawnSync(['plutil', '-extract', keypath, format, '-o', '-', '-'], { stdin: xml })
  return result.success ? new TextDecoder().decode(result.stdout).trim() : null
}

/**
 * `pD.pP` is the ordered pin list: a 1:1 chat's own phone number or email,
 * or an opaque key for a group chat. `pD.pZ` maps those group keys to
 * `{ o, h }`. On this Mac (macOS 26.5.1) `o` matches chat.db's
 * `chat.group_id` / `original_group_id`, not `chat.guid` itself. The two
 * only coincide before a chat has been through the `any;+;<hash>` guid
 * migration.
 */
export function resolvePinned(identifiers: string[], groups: Record<string, PinGroupEntry>): string[] {
  return identifiers.map((identifier) => groups[identifier]?.o ?? identifier)
}

export function pinnedOnMac(): MacPins {
  try {
    const xml = exportDomain(DOMAIN)
    if (!xml) return { identifiers: [], updatedAt: null }

    const identifiersJson = extractKeypath(xml, 'pD.pP', 'json')
    const identifiers = identifiersJson ? (JSON.parse(identifiersJson) as string[]) : []

    const groupsJson = extractKeypath(xml, 'pD.pZ', 'json')
    const groups = groupsJson ? (JSON.parse(groupsJson) as Record<string, PinGroupEntry>) : {}

    const dateRaw = extractKeypath(xml, 'pD.pT', 'raw')
    const updatedAt = dateRaw ? Date.parse(dateRaw) : Number.NaN

    return { identifiers: resolvePinned(identifiers, groups), updatedAt: Number.isFinite(updatedAt) ? updatedAt : null }
  } catch (error) {
    warnOnce(String(error))
    return { identifiers: [], updatedAt: null }
  }
}
