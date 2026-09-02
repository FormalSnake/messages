import { homedir } from 'node:os'
import path from 'node:path'
import { mkdir } from 'node:fs/promises'

export interface ServerConfig {
  url: string
  password: string
}

export interface FindMyConfig {
  url: string
  token: string
}

export interface Config {
  server: ServerConfig | null
  /** Font family override. Defaults per platform in the theme. */
  font?: string
  notifications: boolean
  /** Run against the built-in fixtures instead of a server. */
  demo: boolean
  /** Pinned and muted state lives here; chat.db has no per-client flags. */
  chats: Record<string, { pinned?: boolean; muted?: boolean }>
  /** Address of the `@messages/mac-agent` on the Mac, for Find My locations. */
  findMy?: FindMyConfig
}

const home = homedir()
const platform = typeof process !== 'undefined' ? process.platform : 'linux'

export const configDir =
  process.env.XDG_CONFIG_HOME?.length ? path.join(process.env.XDG_CONFIG_HOME, 'messages') : path.join(home, '.config', 'messages')

export const cacheDir =
  process.env.XDG_CACHE_HOME?.length
    ? path.join(process.env.XDG_CACHE_HOME, 'messages')
    : platform === 'darwin'
      ? path.join(home, 'Library', 'Caches', 'messages')
      : path.join(home, '.cache', 'messages')

export const attachmentsDir = path.join(cacheDir, 'attachments')
export const configFile = path.join(configDir, 'config.json')

const defaults: Config = { server: null, notifications: true, demo: false, chats: {} }

export async function loadConfig(): Promise<Config> {
  let stored: Partial<Config> = {}
  const file = Bun.file(configFile)
  if (await file.exists()) {
    try {
      stored = (await file.json()) as Partial<Config>
    } catch (error) {
      console.error(`config: cannot parse ${configFile}: ${String(error)}`)
    }
  }
  const config: Config = { ...defaults, ...stored }
  const url = process.env.MESSAGES_SERVER_URL
  const password = process.env.MESSAGES_SERVER_PASSWORD
  if (url && password) config.server = { url, password }
  if (process.env.MESSAGES_FONT) config.font = process.env.MESSAGES_FONT
  if (process.env.MESSAGES_DEMO === '1') config.demo = true
  const findMyUrl = process.env.MESSAGES_FINDMY_URL
  const findMyToken = process.env.MESSAGES_FINDMY_TOKEN
  if (findMyUrl && findMyToken) config.findMy = { url: findMyUrl, token: findMyToken }
  return config
}

export async function saveConfig(patch: Partial<Config>): Promise<Config> {
  const current = await loadConfig()
  const next: Config = { ...current, ...patch }
  await mkdir(configDir, { recursive: true, mode: 0o700 })
  await Bun.write(configFile, `${JSON.stringify(next, null, 2)}\n`, { mode: 0o600 })
  return next
}

export async function ensureCacheDirs(): Promise<void> {
  await mkdir(attachmentsDir, { recursive: true })
}
