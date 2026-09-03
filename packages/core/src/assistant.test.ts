import { afterEach, describe, expect, test, vi } from 'vitest'
import { CanaryLLMClient, CanaryLLMError, DEFAULT_CANARYLLM_MODEL, type SummarizeMessage } from './assistant'

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('summarize', () => {
  test('sends a system + user message and returns the completion text', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { choices: [{ message: { content: '  They agreed on Friday.  ' } }] }))
    vi.stubGlobal('fetch', fetchMock)
    const client = new CanaryLLMClient({ apiKey: 'test-key' })
    const messages: SummarizeMessage[] = [
      { sender: 'Alice', text: 'Still on for Friday?', date: 1, fromMe: false },
      { sender: 'Me', text: 'Yes, works for me.', date: 2, fromMe: true },
    ]

    const result = await client.summarize(messages)

    expect(result).toBe('They agreed on Friday.')
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toBe('https://canaryllm.canarycoders.es/v1/chat/completions')
    expect(init.headers).toMatchObject({ authorization: 'Bearer test-key' })
    const body = JSON.parse(init.body as string)
    expect(body.model).toBe(DEFAULT_CANARYLLM_MODEL)
    expect(body.messages[0].role).toBe('system')
    expect(body.messages[1].content).toContain('Alice')
    expect(body.messages[1].content).toContain('Still on for Friday?')
  })

  test('never sends more than the last 200 messages', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { choices: [{ message: { content: 'ok' } }] }))
    vi.stubGlobal('fetch', fetchMock)
    const client = new CanaryLLMClient({ apiKey: 'test-key' })
    const messages: SummarizeMessage[] = Array.from({ length: 250 }, (_, index) => ({ sender: 'Alice', text: `message ${index}`, date: index, fromMe: false }))

    await client.summarize(messages)

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    const body = JSON.parse(init.body as string)
    const transcript = body.messages[1].content as string
    expect(transcript).not.toContain('message 0\n')
    expect(transcript).toContain('message 249')
    expect(transcript.split('\n')).toHaveLength(200)
  })

  test('uses the configured model', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { choices: [{ message: { content: 'ok' } }] }))
    vi.stubGlobal('fetch', fetchMock)
    const client = new CanaryLLMClient({ apiKey: 'test-key', model: 'gemini/gemini-2.5-flash' })

    await client.summarize([{ sender: 'Alice', text: 'hi', date: 1, fromMe: false }])

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string).model).toBe('gemini/gemini-2.5-flash')
  })

  test('throws when the gateway returns an error status', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(500, { error: 'boom' })))
    const client = new CanaryLLMClient({ apiKey: 'test-key' })

    await expect(client.summarize([{ sender: 'Alice', text: 'hi', date: 1, fromMe: false }])).rejects.toThrow(CanaryLLMError)
  })

  test('throws when the completion has no content', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(200, { choices: [] })))
    const client = new CanaryLLMClient({ apiKey: 'test-key' })

    await expect(client.summarize([{ sender: 'Alice', text: 'hi', date: 1, fromMe: false }])).rejects.toThrow(CanaryLLMError)
  })
})

describe('translate', () => {
  test('sends the target language in the system prompt and the text as the user message', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { choices: [{ message: { content: 'Hola' } }] }))
    vi.stubGlobal('fetch', fetchMock)
    const client = new CanaryLLMClient({ apiKey: 'test-key' })

    const result = await client.translate('Hello', 'Spanish')

    expect(result).toBe('Hola')
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    const body = JSON.parse(init.body as string)
    expect(body.messages[0].content).toContain('Spanish')
    expect(body.messages[1].content).toBe('Hello')
  })
})

describe('transcribe', () => {
  // Real timers: the client polls on a fixed 2s interval and this exercises that wait for real.
  test(
    'submits then polls until the job completes',
    async () => {
      const audioPath = new URL(import.meta.url).pathname // any file that exists, its bytes are never inspected here
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(jsonResponse(200, { success: true, data: { queueId: 'q1', status: 'queued' } }))
        .mockResolvedValueOnce(new Response(JSON.stringify({ data: { status: 'processing' } }), { status: 202 }))
        .mockResolvedValueOnce(jsonResponse(200, { data: { status: 'completed', result: { text: 'hello there' } } }))
      vi.stubGlobal('fetch', fetchMock)
      const client = new CanaryLLMClient({ apiKey: 'test-key' })

      const result = await client.transcribe(audioPath)

      expect(result).toBe('hello there')
      expect(fetchMock).toHaveBeenCalledTimes(3)
      const [submitUrl, submitInit] = fetchMock.mock.calls[0] as [string, RequestInit]
      expect(submitUrl).toBe('https://canaryllm.canarycoders.es/api/llm/transcribe')
      expect(JSON.parse(submitInit.body as string)).toMatchObject({ provider: 'elevenlabs' })
      const [pollUrl] = fetchMock.mock.calls[1] as [string, RequestInit]
      expect(pollUrl).toBe('https://canaryllm.canarycoders.es/api/llm/queue/result')
    },
    10_000,
  )

  test(
    'throws when the job errors out on the gateway',
    async () => {
      const audioPath = new URL(import.meta.url).pathname
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(jsonResponse(200, { data: { queueId: 'q1' } }))
        .mockResolvedValueOnce(jsonResponse(200, { data: { status: 'error' } }))
      vi.stubGlobal('fetch', fetchMock)
      const client = new CanaryLLMClient({ apiKey: 'test-key' })

      await expect(client.transcribe(audioPath)).rejects.toThrow(CanaryLLMError)
    },
    10_000,
  )
})
