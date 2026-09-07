import { afterEach, describe, expect, it, vi } from 'vitest'
import { AgentStreamUnsupported, MacAgentClient, type FindMySnapshot } from './agent'

const config = { url: 'http://mac.local:1236', token: 'tok' }

/** A response whose body arrives in the given pieces, so a test can split one message across two reads. */
function sseResponse(chunks: string[], status = 200): Response {
  const encoder = new TextEncoder()
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk))
      controller.close()
    },
  })
  return { ok: status < 400, status, body } as unknown as Response
}

function snapshotJson(latitude: number): string {
  return JSON.stringify({ friends: [{ id: 'f1', addresses: ['+34600111222'], latitude, longitude: -15.4, timestamp: 1, isSharing: true }], devices: null, updatedAt: 2 })
}

async function collect(chunks: string[]): Promise<FindMySnapshot[]> {
  vi.stubGlobal('fetch', vi.fn(async () => sseResponse(chunks)))
  const seen: FindMySnapshot[] = []
  await new MacAgentClient(config).streamFindMy((snapshot) => seen.push(snapshot), new AbortController().signal)
  return seen
}

describe('streamFindMy', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('sends the token and asks for an event stream', async () => {
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => sseResponse([]))
    vi.stubGlobal('fetch', fetchMock)
    await new MacAgentClient(config).streamFindMy(() => {}, new AbortController().signal)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('http://mac.local:1236/findmy/stream')
    expect(fetchMock.mock.calls[0]?.[1]?.headers).toMatchObject({ authorization: 'Bearer tok', accept: 'text/event-stream' })
  })

  it('reassembles a message split across reads', async () => {
    const message = `data: ${snapshotJson(28.1)}\n\n`
    const seen = await collect([message.slice(0, 30), message.slice(30, 60), message.slice(60)])
    expect(seen).toHaveLength(1)
    expect(seen[0]?.friends?.[0]?.latitude).toBe(28.1)
  })

  it('reads two messages out of one read and skips the keepalive between them', async () => {
    const seen = await collect([`data: ${snapshotJson(28.1)}\n\n: keepalive\n\ndata: ${snapshotJson(28.2)}\n\n`])
    expect(seen.map((snapshot) => snapshot.friends?.[0]?.latitude)).toEqual([28.1, 28.2])
  })

  it('drops a message it cannot parse and keeps reading', async () => {
    const seen = await collect([`data: {"friends":\n\ndata: ${snapshotJson(28.3)}\n\n`])
    expect(seen.map((snapshot) => snapshot.friends?.[0]?.latitude)).toEqual([28.3])
  })

  it('tells an agent without the endpoint apart from one that is down', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([], 404)))
    await expect(new MacAgentClient(config).streamFindMy(() => {}, new AbortController().signal)).rejects.toBeInstanceOf(AgentStreamUnsupported)

    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([], 503)))
    await expect(new MacAgentClient(config).streamFindMy(() => {}, new AbortController().signal)).rejects.toThrow(/503/)
  })
})
