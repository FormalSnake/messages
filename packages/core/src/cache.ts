import path from 'node:path'
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { UNKNOWN_MIME, type Chat, type Contact, type Message } from './model'

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

let tempCounter = 0

/**
 * Last known chats and threads on disk, so the window paints before the
 * server answers. Writes are debounced and atomic; a corrupt file is ignored.
 */
export class StateCache {
  private readonly file: string
  private readonly temp: string
  private timer: ReturnType<typeof setTimeout> | null = null
  private pending: (() => CachedState) | null = null
  private writing: Promise<void> = Promise.resolve()

  constructor(cacheDir: string) {
    this.file = path.join(cacheDir, 'state.json')
    tempCounter += 1
    // A reconnect builds a new store, and its cache, beside the old one; two
    // of them writing the same temp file lose a snapshot to a failed rename.
    this.temp = `${this.file}.${process.pid}-${tempCounter}.tmp`
  }

  async load(): Promise<CachedState | null> {
    try {
      const parsed = JSON.parse(await readFile(this.file, 'utf8')) as Partial<CachedState>
      if (parsed.version !== 1 || !Array.isArray(parsed.chats)) return null
      return {
        version: 1,
        savedAt: parsed.savedAt ?? 0,
        selectedChat: parsed.selectedChat ?? null,
        chats: parsed.chats,
        messages: sound(parsed.messages ?? {}),
        contacts: parsed.contacts ?? [],
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return null
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
    if (snapshot) this.writing = this.writing.then(() => this.write(snapshot())).catch((error) => console.error(`cache: ${String(error)}`))
    // With nothing pending there can still be a write in flight from the
    // timer, and stop() has to leave the file complete.
    await this.writing
  }

  private async write(state: CachedState): Promise<void> {
    await mkdir(path.dirname(this.file), { recursive: true, mode: 0o700 })
    await writeFile(this.temp, JSON.stringify(state), { mode: 0o600 })
    await rename(this.temp, this.file)
  }
}

/**
 * The file is whatever an older build wrote, and one of those wrote through a
 * `mime: string` that the server had left null. Every reader calls
 * `mime.startsWith`, so a single one of those took the window down as soon as
 * the details panel listed it.
 */
function sound(messages: Record<string, Message[]>): Record<string, Message[]> {
  const out: Record<string, Message[]> = {}
  for (const [guid, list] of Object.entries(messages)) {
    out[guid] = list.map((message) =>
      message.attachments?.some((attachment) => typeof attachment.mime !== 'string')
        ? { ...message, attachments: message.attachments.map((attachment) => (typeof attachment.mime === 'string' ? attachment : { ...attachment, mime: UNKNOWN_MIME })) }
        : { ...message, attachments: message.attachments ?? [] },
    )
  }
  return out
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
