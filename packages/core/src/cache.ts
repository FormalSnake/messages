import path from 'node:path'
import { mkdir, rename } from 'node:fs/promises'
import type { Chat, Contact, Message } from './model'

export interface CachedState {
  version: 1
  savedAt: number
  selectedChat: string | null
  chats: Chat[]
  messages: Record<string, Message[]>
  contacts: Contact[]
}

const MESSAGES_PER_CHAT = 100
const SAVE_DELAY_MS = 2000

/**
 * Last known chats and threads on disk, so the window paints before the
 * server answers. Writes are debounced and atomic; a corrupt file is ignored.
 */
export class StateCache {
  private readonly file: string
  private timer: ReturnType<typeof setTimeout> | null = null
  private pending: (() => CachedState) | null = null
  private writing: Promise<void> = Promise.resolve()

  constructor(cacheDir: string) {
    this.file = path.join(cacheDir, 'state.json')
  }

  async load(): Promise<CachedState | null> {
    try {
      const file = Bun.file(this.file)
      if (!(await file.exists())) return null
      const parsed = (await file.json()) as Partial<CachedState>
      if (parsed.version !== 1 || !Array.isArray(parsed.chats)) return null
      return {
        version: 1,
        savedAt: parsed.savedAt ?? 0,
        selectedChat: parsed.selectedChat ?? null,
        chats: parsed.chats,
        messages: parsed.messages ?? {},
        contacts: parsed.contacts ?? [],
      }
    } catch (error) {
      console.error(`cache: ignoring ${this.file}: ${String(error)}`)
      return null
    }
  }

  /** Schedules a write; the snapshot is taken when the timer fires, not now. */
  schedule(snapshot: () => CachedState): void {
    this.pending = snapshot
    if (this.timer) return
    this.timer = setTimeout(() => {
      this.timer = null
      void this.flush()
    }, SAVE_DELAY_MS)
  }

  async flush(): Promise<void> {
    const snapshot = this.pending
    this.pending = null
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
    if (!snapshot) return
    this.writing = this.writing.then(() => this.write(snapshot())).catch((error) => console.error(`cache: ${String(error)}`))
    await this.writing
  }

  private async write(state: CachedState): Promise<void> {
    await mkdir(path.dirname(this.file), { recursive: true, mode: 0o700 })
    const temp = `${this.file}.${process.pid}.tmp`
    await Bun.write(temp, JSON.stringify(state), { mode: 0o600 })
    await rename(temp, this.file)
  }
}

export function snapshotForCache(state: { selectedChat: string | null; chats: Chat[]; messages: Record<string, Message[]>; contacts: Contact[] }): CachedState {
  const messages: Record<string, Message[]> = {}
  for (const [guid, list] of Object.entries(state.messages)) {
    // Drop in-flight sends; they would come back as ghosts with no server guid.
    const settled = list.filter((message) => !(message.tempGuid && message.guid === message.tempGuid))
    if (settled.length) messages[guid] = settled.slice(-MESSAGES_PER_CHAT)
  }
  return { version: 1, savedAt: Date.now(), selectedChat: state.selectedChat, chats: state.chats, messages, contacts: state.contacts }
}
