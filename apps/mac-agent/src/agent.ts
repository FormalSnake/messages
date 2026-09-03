/**
 * Runs on the Mac. Serves the friend and device locations `findmy/index.ts`
 * decrypts, over HTTP, so `packages/core`'s `FindMyClient` can reach them
 * from the Linux desktop.
 */

import { randomBytes } from 'node:crypto'
import { mkdir } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { cachedDevices, cachedFriends, keyAvailability } from './findmy/index'
import { pinnedOnMac } from './pinning'
import { loadPrefs, updatePrefs } from './prefs'

const MAX_PREFS_BODY_BYTES = 1024 * 1024

class BodyTooLargeError extends Error {}

interface AgentConfig {
  token: string
  port: number
  host: string
  /** When true, `/findmy/refresh` launches Find My to pull fresh caches. Off by default: it steals focus briefly. */
  refresh?: boolean
}

const configDir = path.join(homedir(), '.config', 'messages')
const configPath = path.join(configDir, 'agent.json')

async function loadOrCreateConfig(): Promise<AgentConfig> {
  const file = Bun.file(configPath)
  if (await file.exists()) {
    const stored = (await file.json()) as Partial<AgentConfig>
    if (stored.token) return { token: stored.token, port: stored.port ?? 1236, host: stored.host ?? '0.0.0.0', refresh: stored.refresh ?? false }
  }
  const config: AgentConfig = { token: randomBytes(32).toString('hex'), port: 1236, host: '0.0.0.0', refresh: false }
  await mkdir(configDir, { recursive: true, mode: 0o700 })
  await Bun.write(configPath, `${JSON.stringify(config, null, 2)}\n`, { mode: 0o600 })
  console.log(`mac-agent: wrote a new config (with a fresh token) to ${configPath}`)
  return config
}

/** Launching and quitting Find My is the only known way to make it refresh its on-disk caches; see FindMyRefresher.swift. */
async function refreshFindMy(): Promise<void> {
  Bun.spawnSync(['open', '-g', '-a', 'FindMy'])
  await Bun.sleep(5000)
  Bun.spawnSync(['osascript', '-e', 'tell application "FindMy" to quit'])
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

function prefsResponse(prefs: { chats: unknown }): Response {
  const mac = pinnedOnMac()
  return json({ chats: prefs.chats, macPinned: mac.identifiers, macPinnedAt: mac.updatedAt })
}

export async function startAgent(): Promise<ReturnType<typeof Bun.serve>> {
  const config = await loadOrCreateConfig()

  const server = Bun.serve({
    hostname: config.host,
    port: config.port,
    async fetch(request) {
      const url = new URL(request.url)

      if (request.method === 'GET' && url.pathname === '/health') return json({ ok: true, keys: keyAvailability(), prefs: true })

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
          return prefsResponse(await updatePrefs(configDir, { chats: chats as Record<string, unknown> }))
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

      if (request.method === 'POST' && url.pathname === '/findmy/refresh') {
        if (!config.refresh) return json({ refreshed: false })
        await refreshFindMy()
        return json({ refreshed: true })
      }

      return json({ error: 'not found' }, { status: 404 })
    },
  })

  console.log(`mac-agent: listening on ${config.host}:${config.port}`)
  return server
}

if (import.meta.main) await startAgent()
