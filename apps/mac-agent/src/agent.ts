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

export async function startAgent(): Promise<ReturnType<typeof Bun.serve>> {
  const config = await loadOrCreateConfig()

  const server = Bun.serve({
    hostname: config.host,
    port: config.port,
    async fetch(request) {
      const url = new URL(request.url)

      if (request.method === 'GET' && url.pathname === '/health') return json({ ok: true, keys: keyAvailability() })

      if (!authorized(request, config.token)) return json({ error: 'unauthorized' }, { status: 401 })

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
