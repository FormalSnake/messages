/**
 * Runs on the Mac. Serves the friend and device locations `findmy/index.ts`
 * decrypts, over HTTP, so `packages/core`'s `FindMyClient` can reach them
 * from the Linux desktop.
 */

import { randomBytes } from 'node:crypto'
import { mkdir } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { subscribeFindMy } from './findmy/feed'
import { cachedDevices, cachedFriends, keyAvailability } from './findmy/index'
import { findMyRunning, keepFindMyOpen } from './findmy/refresher'
import { pinnedOnMac } from './pinning'
import { loadPrefs, updatePrefs } from './prefs'

const MAX_PREFS_BODY_BYTES = 1024 * 1024

class BodyTooLargeError extends Error {}

interface AgentConfig {
  token: string
  port: number
  host: string
  /** Keeps FindMy.app running hidden, which is the only thing that keeps its caches current. Set false to leave the app alone. */
  keepFindMyOpen?: boolean
}

const configDir = path.join(homedir(), '.config', 'messages')
const configPath = path.join(configDir, 'agent.json')

async function loadOrCreateConfig(): Promise<AgentConfig> {
  const file = Bun.file(configPath)
  if (await file.exists()) {
    const stored = (await file.json()) as Partial<AgentConfig>
    if (stored.token) return { token: stored.token, port: stored.port ?? 1236, host: stored.host ?? '0.0.0.0', keepFindMyOpen: stored.keepFindMyOpen ?? true }
  }
  const config: AgentConfig = { token: randomBytes(32).toString('hex'), port: 1236, host: '0.0.0.0', keepFindMyOpen: true }
  await mkdir(configDir, { recursive: true, mode: 0o700 })
  await Bun.write(configPath, `${JSON.stringify(config, null, 2)}\n`, { mode: 0o600 })
  console.log(`mac-agent: wrote a new config (with a fresh token) to ${configPath}`)
  return config
}

function json(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), { ...init, headers: { 'content-type': 'application/json', ...init.headers } })
}

function authorized(request: Request, token: string): boolean {
  return request.headers.get('authorization') === `Bearer ${token}`
}

function errorResponse(error: unknown): Response {
  return json({ error: error instanceof Error ? error.message : String(error) }, { status: 503 })
}

/** Reads the body as text, aborting once it exceeds `maxBytes` rather than trusting `content-length`. */
async function readCappedBody(request: Request, maxBytes: number): Promise<string> {
  const reader = request.body?.getReader()
  if (!reader) return ''
  const chunks: Uint8Array[] = []
  let total = 0
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    total += value.byteLength
    if (total > maxBytes) {
      await reader.cancel()
      throw new BodyTooLargeError('request body exceeds 1 MB')
    }
    chunks.push(value)
  }
  const buffer = new Uint8Array(total)
  let offset = 0
  for (const chunk of chunks) {
    buffer.set(chunk, offset)
    offset += chunk.byteLength
  }
  return new TextDecoder().decode(buffer)
}

/** A comment line often enough that a connection dropped in between is noticed rather than sat on. */
const KEEPALIVE_MS = 20_000

/** Server-sent events: one message per Find My write, plus the current state on connect. */
function findMyStream(): Response {
  const encoder = new TextEncoder()
  let unsubscribe = () => {}
  let keepalive: ReturnType<typeof setInterval> | null = null

  const stream = new ReadableStream({
    start(controller) {
      const write = (chunk: string) => {
        try {
          controller.enqueue(encoder.encode(chunk))
        } catch {
          // The client hung up between the write and `cancel`; the unsubscribe below is enough.
        }
      }
      unsubscribe = subscribeFindMy((snapshot) => write(`data: ${JSON.stringify(snapshot)}\n\n`))
      keepalive = setInterval(() => write(': keepalive\n\n'), KEEPALIVE_MS)
    },
    cancel() {
      unsubscribe()
      if (keepalive) clearInterval(keepalive)
    },
  })

  return new Response(stream, {
    headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' },
  })
}

function prefsResponse(prefs: { chats: unknown; gifs: unknown }): Response {
  const mac = pinnedOnMac()
  return json({ chats: prefs.chats, gifs: prefs.gifs, macPinned: mac.identifiers, macPinnedAt: mac.updatedAt })
}

export async function startAgent(): Promise<ReturnType<typeof Bun.serve>> {
  const config = await loadOrCreateConfig()
  if (config.keepFindMyOpen !== false) keepFindMyOpen()

  const server = Bun.serve({
    hostname: config.host,
    port: config.port,
    async fetch(request, server) {
      const url = new URL(request.url)

      if (request.method === 'GET' && url.pathname === '/health') return json({ ok: true, keys: keyAvailability(), prefs: true, findMyOpen: findMyRunning() })

      if (!authorized(request, config.token)) return json({ error: 'unauthorized' }, { status: 401 })

      if (request.method === 'GET' && url.pathname === '/prefs') {
        try {
          return prefsResponse(await loadPrefs(configDir))
        } catch (error) {
          return errorResponse(error)
        }
      }

      if (request.method === 'PUT' && url.pathname === '/prefs') {
        try {
          const body = JSON.parse(await readCappedBody(request, MAX_PREFS_BODY_BYTES)) as unknown
          if (typeof body !== 'object' || body === null || Array.isArray(body)) return json({ error: 'expected an object with a chats field' }, { status: 400 })
          const chats = (body as { chats?: unknown }).chats
          if (typeof chats !== 'object' || chats === null || Array.isArray(chats)) return json({ error: 'expected an object with a chats field' }, { status: 400 })
          // An older client sends no `gifs` field at all; treat that as "no change" rather than wiping the server's favorites.
          const gifsField = (body as { gifs?: unknown }).gifs
          const gifs = typeof gifsField === 'object' && gifsField !== null && !Array.isArray(gifsField) ? (gifsField as Record<string, unknown>) : {}
          return prefsResponse(await updatePrefs(configDir, { chats: chats as Record<string, unknown>, gifs }))
        } catch (error) {
          if (error instanceof BodyTooLargeError) return json({ error: error.message }, { status: 413 })
          return json({ error: 'invalid JSON body' }, { status: 400 })
        }
      }

      if (request.method === 'GET' && url.pathname === '/findmy/friends') {
        try {
          return json(await cachedFriends())
        } catch (error) {
          return errorResponse(error)
        }
      }

      if (request.method === 'GET' && url.pathname === '/findmy/devices') {
        try {
          return json(cachedDevices())
        } catch (error) {
          return errorResponse(error)
        }
      }

      if (request.method === 'GET' && url.pathname === '/findmy/stream') {
        // A stream between two Find My writes reads as idle, and Bun closes an idle connection after ten seconds.
        server.timeout(request, 0)
        return findMyStream()
      }

      return json({ error: 'not found' }, { status: 404 })
    },
  })

  console.log(`mac-agent: listening on ${config.host}:${config.port}`)
  return server
}

if (import.meta.main) await startAgent()
