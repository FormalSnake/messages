/**
 * Client for the CanaryLLM gateway (canaryllm.canarycoders.es). Summaries and
 * translation go through the synchronous `/v1/chat/completions` compat
 * endpoint. Transcription only exists as the queued native
 * `/api/llm/transcribe` endpoint, so it submits a job and polls for it.
 *
 * Every call here is triggered by a click in the UI; nothing in this file
 * schedules or retries a request on its own.
 */

import { readFile } from 'node:fs/promises'

const DEFAULT_BASE_URL = 'https://canaryllm.canarycoders.es'
/** Cheapest Gemini flash-lite model the gateway offers, plenty for a short summary or a translation. */
export const DEFAULT_CANARYLLM_MODEL = 'gemini/gemini-2.5-flash-lite'
const TRANSCRIBE_MODEL = 'scribe_v2'

const CHAT_TIMEOUT_MS = 30_000
const TRANSCRIBE_TIMEOUT_MS = 120_000
const TRANSCRIBE_POLL_INTERVAL_MS = 2_000
/** Ceiling on poll attempts so a stuck job cannot poll forever even if the clock check is skewed. */
const TRANSCRIBE_MAX_POLLS = 90

/** Never send more than this many messages, or any attachment bytes, off the machine. */
const MAX_SUMMARY_MESSAGES = 200

export interface CanaryLLMClientOptions {
  apiKey: string
  model?: string
  baseUrl?: string
}

export interface RequestOptions {
  signal?: AbortSignal
}

export interface SummarizeMessage {
  /** Display name of the sender, or "Me" for outgoing messages. Never a raw address. */
  sender: string
  text: string
  date: number
  fromMe: boolean
}

export class CanaryLLMError extends Error {}

function mimeFromPath(path: string): string {
  const ext = path.slice(path.lastIndexOf('.') + 1).toLowerCase()
  switch (ext) {
    case 'mp3':
      return 'audio/mpeg'
    case 'm4a':
    case 'mp4':
      return 'audio/mp4'
    case 'aac':
      return 'audio/aac'
    case 'wav':
      return 'audio/wav'
    case 'caf':
      return 'audio/x-caf'
    case 'ogg':
      return 'audio/ogg'
    default:
      return 'audio/mp4'
  }
}

async function withTimeout<T>(ms: number, external: AbortSignal | undefined, run: (signal: AbortSignal) => Promise<T>): Promise<T> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), ms)
  const onExternalAbort = () => controller.abort()
  external?.addEventListener('abort', onExternalAbort)
  try {
    return await run(controller.signal)
  } finally {
    clearTimeout(timer)
    external?.removeEventListener('abort', onExternalAbort)
  }
}

interface ChatCompletion {
  choices?: Array<{ message?: { content?: string } }>
}

function formatTranscript(messages: SummarizeMessage[]): string {
  return messages
    .map((message) => `${message.fromMe ? 'Me' : message.sender} (${new Date(message.date).toLocaleString()}): ${message.text || '[attachment, no text]'}`)
    .join('\n')
}

export class CanaryLLMClient {
  private readonly apiKey: string
  private readonly model: string
  private readonly baseUrl: string

  constructor(options: CanaryLLMClientOptions) {
    this.apiKey = options.apiKey
    this.model = options.model ?? DEFAULT_CANARYLLM_MODEL
    this.baseUrl = options.baseUrl ?? DEFAULT_BASE_URL
  }

  private async chat(system: string, user: string, options: RequestOptions = {}): Promise<string> {
    return withTimeout(CHAT_TIMEOUT_MS, options.signal, async (signal) => {
      const response = await fetch(`${this.baseUrl}/v1/chat/completions`, {
        method: 'POST',
        headers: { authorization: `Bearer ${this.apiKey}`, 'content-type': 'application/json' },
        body: JSON.stringify({
          model: this.model,
          messages: [
            { role: 'system', content: system },
            { role: 'user', content: user },
          ],
          max_tokens: 500,
          temperature: 0.3,
        }),
        signal,
      })
      if (!response.ok) throw new CanaryLLMError(`canaryllm: chat completions returned ${response.status}`)
      const body = (await response.json()) as ChatCompletion
      const content = body.choices?.[0]?.message?.content?.trim()
      if (!content) throw new CanaryLLMError('canaryllm: chat completions returned no content')
      return content
    })
  }

  /** Summarizes what happened since the caller's own last message (or the last 50), oldest first. */
  async summarize(messages: SummarizeMessage[], options: RequestOptions = {}): Promise<string> {
    const capped = messages.slice(-MAX_SUMMARY_MESSAGES)
    const system =
      'You summarize iMessage conversations for the person reading them. Reply in sentence case, 1-3 short sentences, covering what was said or decided. No preamble, no bullet points, no repeating the instructions.'
    return this.chat(system, formatTranscript(capped), options)
  }

  async translate(text: string, targetLanguage: string, options: RequestOptions = {}): Promise<string> {
    const system = `Translate the user's message into ${targetLanguage}. Reply with only the translation, nothing else.`
    return this.chat(system, text, options)
  }

  /** Submits the audio to the gateway's queued STT endpoint and polls for the transcript, capped at 120s total. */
  async transcribe(audioPath: string, options: RequestOptions = {}): Promise<string> {
    return withTimeout(TRANSCRIBE_TIMEOUT_MS, options.signal, async (signal) => {
      const bytes = await readFile(audioPath)
      const audio = bytes.toString('base64')
      const submitResponse = await fetch(`${this.baseUrl}/api/llm/transcribe`, {
        method: 'POST',
        headers: { authorization: `Bearer ${this.apiKey}`, 'content-type': 'application/json' },
        body: JSON.stringify({ provider: 'elevenlabs', model: TRANSCRIBE_MODEL, audio, mimeType: mimeFromPath(audioPath) }),
        signal,
      })
      if (!submitResponse.ok) throw new CanaryLLMError(`canaryllm: transcribe submit returned ${submitResponse.status}`)
      const submitBody = (await submitResponse.json()) as { data?: { queueId?: string } }
      const queueId = submitBody.data?.queueId
      if (!queueId) throw new CanaryLLMError('canaryllm: transcribe submit returned no queueId')

      for (let attempt = 0; attempt < TRANSCRIBE_MAX_POLLS; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, TRANSCRIBE_POLL_INTERVAL_MS))
        const pollResponse = await fetch(`${this.baseUrl}/api/llm/queue/result`, {
          method: 'POST',
          headers: { authorization: `Bearer ${this.apiKey}`, 'content-type': 'application/json' },
          body: JSON.stringify({ queueId }),
          signal,
        })
        if (pollResponse.status === 202) continue
        if (!pollResponse.ok) throw new CanaryLLMError(`canaryllm: transcribe poll returned ${pollResponse.status}`)
        const pollBody = (await pollResponse.json()) as { data?: { status?: string; result?: { text?: string; transcript?: string } } }
        const status = pollBody.data?.status
        if (status === 'error' || status === 'cancelled') throw new CanaryLLMError(`canaryllm: transcribe job ${status}`)
        const text = pollBody.data?.result?.text ?? pollBody.data?.result?.transcript
        if (text) return text
        throw new CanaryLLMError('canaryllm: transcribe completed with no text')
      }
      throw new CanaryLLMError('canaryllm: transcribe timed out waiting for the queue')
    })
  }
}
