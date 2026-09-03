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

/** A Mac pin is overridden by a client change made after the Mac's list was last edited. */
export function isPinned(prefs: ChatPrefs | undefined): boolean {
  if (!prefs) return false
  if (prefs.macPinned && (!prefs.updatedAt || (prefs.macPinnedAt ?? 0) > prefs.updatedAt)) return true
  return prefs.pinned ?? false
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

  /** Pushes this client's entries and gets back the merged set plus the Mac's own pins. */
  syncPrefs(chats: Record<string, ChatPrefs>, gifs: Record<string, GifFavorite> = {}): Promise<SharedPrefs> {
    return fetchJson(this.endpoint('/prefs'), {
      method: 'PUT',
      headers: { ...this.authHeaders(), 'content-type': 'application/json' },
      body: JSON.stringify({ chats, gifs }),
    })
  }
}
