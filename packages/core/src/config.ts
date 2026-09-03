import { homedir } from 'node:os'
import path from 'node:path'
import { mkdir } from 'node:fs/promises'
import type { AgentConfig, ChatPrefs } from './agent'
import type { GifFavorite } from './gifs'

export interface ServerConfig {
  url: string
  password: string
}

export interface KlipyConfig {
  apiKey: string
}

export interface CanaryLLMConfig {
  apiKey: string
  /** Gateway model id, `provider/model` format. Defaults to a cheap Gemini flash-lite model. */
  model?: string
  /** Target language for translation. Defaults to the system locale's language, then English. */
  language?: string
}

export interface Config {
  server: ServerConfig | null
  /** Font family override. Defaults per platform in the theme. */
  font?: string
  notifications: boolean
  /** Run against the built-in fixtures instead of a server. */
  demo: boolean
  /** Pinned and muted state lives here; chat.db has no per-client flags. */
  chats: Record<string, ChatPrefs>
  /** Address of the `@messages/mac-agent` on the Mac: Find My locations and prefs shared between clients. */
  agent?: AgentConfig
  /** Klipy GIF API key (see `gifs.ts`). The composer's GIF button only shows up when this is set. */
  klipy?: KlipyConfig
  /** Favorited GIFs, keyed by gif id; synced through the Mac agent same as `chats`. */
  gifFavorites?: Record<string, GifFavorite>
  /** CanaryLLM gateway for the summarize, translate and transcribe buttons. Unset hides all three. */
  canaryllm?: CanaryLLMConfig
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

/** What is on disk, and nothing else. */
async function readStoredConfig(): Promise<Partial<Config>> {
  const file = Bun.file(configFile)
  if (!(await file.exists())) return {}
  try {
    return (await file.json()) as Partial<Config>
  } catch (error) {
    console.error(`config: cannot parse ${configFile}: ${String(error)}`)
    return {}
  }
}

export async function loadConfig(): Promise<Config> {
  const config: Config = { ...defaults, ...(await readStoredConfig()) }
  const url = process.env.MESSAGES_SERVER_URL
  const password = process.env.MESSAGES_SERVER_PASSWORD
  if (url && password) config.server = { url, password }
  if (process.env.MESSAGES_FONT) config.font = process.env.MESSAGES_FONT
  if (process.env.MESSAGES_DEMO === '1') config.demo = true
  const agentUrl = process.env.MESSAGES_AGENT_URL
  const agentToken = process.env.MESSAGES_AGENT_TOKEN
  if (agentUrl && agentToken) config.agent = { url: agentUrl, token: agentToken }
  const klipyKey = process.env.MESSAGES_KLIPY_KEY
  if (klipyKey) config.klipy = { apiKey: klipyKey }
  const canaryllmKey = process.env.MESSAGES_CANARYLLM_KEY
  if (canaryllmKey) config.canaryllm = { ...config.canaryllm, apiKey: canaryllmKey }
  return config
}

// Merges into the file as it is, not into the loaded config: the environment
// overrides (a demo run, a server passed by variable) must never be written back.
export async function saveConfig(patch: Partial<Config>): Promise<Config> {
  const next: Config = { ...defaults, ...(await readStoredConfig()), ...patch }
  await mkdir(configDir, { recursive: true, mode: 0o700 })
  await Bun.write(configFile, `${JSON.stringify(next, null, 2)}\n`, { mode: 0o600 })
  return next
}

export async function ensureCacheDirs(): Promise<void> {
  await mkdir(attachmentsDir, { recursive: true })
}
