/**
 * Client for `@messages/mac-agent`, the small HTTP server this repo runs on
 * the Mac next to BlueBubbles. It serves what BlueBubbles cannot: decrypted
 * Find My locations, and the pinned and muted state shared between clients.
 */

import type { DeviceLocation, FriendLocation } from './findmy'
import type { GifFavorite } from './gifs'

export interface AgentConfig {
  url: string
  token: string
}

export interface AgentHealth {
  ok: boolean
  keys: { friends: boolean; fmf: boolean; fmip: boolean }
  /** Set by agents that serve `/prefs`. */
  prefs?: boolean
  /** Whether FindMy.app is running on the Mac. Nothing refreshes its caches while it is not. */
  findMyOpen?: boolean
}

/** One push from `/findmy/stream`. A half the agent could not read arrives as null, so a client keeps what it already had. */
export interface FindMySnapshot {
  friends: FriendLocation[] | null
  devices: DeviceLocation[] | null
  updatedAt: number
}

/** Per-chat client state. chat.db has no per-client flags, so this lives in the config and on the Mac agent. */
export interface ChatPrefs {
  pinned?: boolean
  muted?: boolean
  /** false skips the read receipt when this conversation is marked read; the local unread dot still clears. */
  readReceipts?: boolean
  /** The composer text for this chat, synced between clients. */
  draft?: string
  /** When a client last changed this entry; the newer entry wins between clients. */
  updatedAt?: number
  /** When a client last pinned or unpinned this chat. Only a pin moves it, so a draft or a mute cannot outrank the Mac's list. */
  pinnedAt?: number
  /** Pinned in Messages.app on the Mac, and when that list was last edited. */
  macPinned?: boolean
  macPinnedAt?: number
}

export interface SharedPrefs {
  chats: Record<string, ChatPrefs>
  /** GIF favorites, keyed by gif id. */
  gifs: Record<string, GifFavorite>
  /** Pinned in Messages.app: an address for a one-to-one chat, a chat guid for a group. */
  macPinned: string[]
  macPinnedAt: number | null
}

/** A Mac pin is overridden by a pin this client changed after the Mac's list was last edited, and by nothing else. */
export function isPinned(prefs: ChatPrefs | undefined): boolean {
  if (!prefs) return false
  if (prefs.macPinned && (prefs.macPinnedAt ?? 0) > (prefs.pinnedAt ?? 0)) return true
  return prefs.pinned ?? false
}

/** Thrown when the Mac agent predates `/findmy/stream`, so a client can fall back to polling instead of retrying forever. */
export class AgentStreamUnsupported extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'AgentStreamUnsupported'
  }
}

const TIMEOUT_MS = 10_000

async function fetchJson<T>(url: string, init: RequestInit = {}): Promise<T> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS)
  try {
    const response = await fetch(url, { ...init, signal: controller.signal })
    if (!response.ok) throw new Error(`agent: ${url} returned ${response.status}`)
    return (await response.json()) as T
  } finally {
    clearTimeout(timer)
  }
}

export class MacAgentClient {
  constructor(private readonly options: AgentConfig) {}

  private endpoint(pathname: string): string {
    return new URL(pathname, this.options.url).toString()
  }

  private authHeaders(): Record<string, string> {
    return { authorization: `Bearer ${this.options.token}` }
  }

  health(): Promise<AgentHealth> {
    return fetchJson<AgentHealth>(this.endpoint('/health'))
  }

  friends(): Promise<{ friends: FriendLocation[]; updatedAt: number }> {
    return fetchJson(this.endpoint('/findmy/friends'), { headers: this.authHeaders() })
  }

  devices(): Promise<{ devices: DeviceLocation[]; updatedAt: number }> {
    return fetchJson(this.endpoint('/findmy/devices'), { headers: this.authHeaders() })
  }

  /**
   * Server-sent events: the agent pushes a snapshot as Find My writes one, so
   * a location shows up in seconds instead of on the next poll. Resolves when
   * the agent closes the stream, rejects when it cannot be reached; either way
   * the caller decides whether to reconnect. No timeout, unlike `fetchJson`:
   * the point of the connection is to stay open.
   */
  async streamFindMy(onSnapshot: (snapshot: FindMySnapshot) => void, signal: AbortSignal): Promise<void> {
    const response = await fetch(this.endpoint('/findmy/stream'), { headers: { ...this.authHeaders(), accept: 'text/event-stream' }, signal })
    if (response.status === 404) throw new AgentStreamUnsupported('agent: this Mac agent does not serve /findmy/stream')
    if (!response.ok || !response.body) throw new Error(`agent: /findmy/stream returned ${response.status}`)
    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    for (;;) {
      const { done, value } = await reader.read()
      if (done) return
      buffer += decoder.decode(value, { stream: true })
      // An SSE message ends at a blank line; a `:` line is the agent's keepalive and carries no data.
      for (let end = buffer.indexOf('\n\n'); end !== -1; end = buffer.indexOf('\n\n')) {
        const message = buffer.slice(0, end)
        buffer = buffer.slice(end + 2)
        const data = message
          .split('\n')
          .filter((line) => line.startsWith('data:'))
          .map((line) => line.slice(5).trim())
          .join('')
        if (!data) continue
        try {
          onSnapshot(JSON.parse(data) as FindMySnapshot)
        } catch (error) {
          console.error(`agent: unreadable Find My snapshot: ${String(error)}`)
        }
      }
    }
  }

  /** Pushes this client's entries and gets back the merged set plus the Mac's own pins. */
  syncPrefs(chats: Record<string, ChatPrefs>, gifs: Record<string, GifFavorite> = {}): Promise<SharedPrefs> {
    return fetchJson(this.endpoint('/prefs'), {
      method: 'PUT',
      headers: { ...this.authHeaders(), 'content-type': 'application/json' },
      body: JSON.stringify({ chats, gifs }),
    })
  }
}
