/**
 * Drives the app on the demo fixtures through the GPU test renderer.
 *
 *   bun run test
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { connectTest } from '@gpuix/react/automation'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { DemoTransport, type Config } from '@messages/core'

import { MessagesApp } from './src/ui/app'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const config: Config = { server: null, notifications: false, demo: true, chats: {} }

function mount(overrides: Partial<Config> = {}, size: { width: number; height: number } = { width: 1120, height: 760 }) {
  const { render, renderer } = createTestRoot(size)
  const transport = new DemoTransport()
  render(<MessagesApp config={{ ...config, ...overrides }} saveConfig={async () => undefined} transport={transport} />)
  return { renderer, transport }
}

/** Every chat row on screen, top first, one entry per row. */
async function paintedRows(app: Awaited<ReturnType<typeof connectTest>>): Promise<Array<{ id: string; y: number }>> {
  const rows = new Map<string, number>()
  const walk = (node: { testId?: string; bounds?: { y: number }; children?: unknown[] }) => {
    if (node.testId?.startsWith('chat-') && node.bounds && !rows.has(node.testId)) rows.set(node.testId, Math.round(node.bounds.y))
    for (const child of (node.children ?? []) as Array<Parameters<typeof walk>[0]>) walk(child)
  }
  for (const node of await app.getByTestId('sidebar').all()) walk(node)
  return [...rows].map(([id, y]) => ({ id, y })).sort((a, b) => a.y - b.y)
}

describeNative('messages app', () => {
  it('opens on the most recent conversation and paints its thread', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })
    await app.getByTestId('thread').getByText('bring the charger this time 🔌').waitFor({ timeoutMs: 20_000 })

    const painted = renderer.getPaintedText()
    expect(painted).toContain('Alex Rivera')
    expect(painted).toContain('coffee at 4? the place on valencia')
    expect(painted.some((line) => line.startsWith('Read '))).toBe(true)

    await app.close()
  })

  it('sends a message and shows the reply from the other side', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('draft').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('draft').fill('see you there')
    await app.getByTestId('send').click()
    await app.getByTestId('thread').getByText('see you there').waitFor({ timeoutMs: 10_000 })
    await app.getByTestId('thread').getByText('ha, deal').waitFor({ timeoutMs: 15_000 })

    await app.close()
  })

  it('switches conversations from the sidebar', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('chat-chat240119384759').click()
    await app.getByTestId('thread').getByText('Sunday lunch is at ours, 1pm. Bring the good bread.').waitFor({ timeoutMs: 10_000 })
    expect(await app.getByTestId('thread-title').textContent()).toBe('Family')

    await app.close()
  })

  it('names the Focus on the other end and breaks through it', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('chat-+14155550170').click()
    await app.getByTestId('thread').getByText('Reviewing tonight').waitFor({ timeoutMs: 10_000 })
    expect(await app.getByTestId('thread-title').textContent()).toBe('Ben Okafor')
    await app.getByTestId('thread').getByText('Delivered Quietly').waitFor({ timeoutMs: 10_000 })

    await app.getByTestId('thread').getByText('Notify Anyway').click()
    await app.getByTestId('thread').getByText('Notified').waitFor({ timeoutMs: 10_000 })

    await app.close()
  })

  it('leaves the sidebar where it is when a row halfway down is clicked', async () => {
    // gpui scrolls a List to reveal whatever takes focus, so selecting a row used to jump the list.
    const { renderer } = mount({}, { width: 1120, height: 420 })
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })
    await app.getByTestId('sidebar').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('sidebar').wheel(0, -120)
    const before = await paintedRows(app)
    expect(before.length).toBeGreaterThan(3)

    // Near the top of the window, with room below: the reveal scroll had the most to take away here.
    const row = before[1]!
    await app.getByTestId(row.id).click()
    await app.getByTestId('thread').waitFor({ timeoutMs: 10_000 })

    expect(await paintedRows(app)).toEqual(before)

    await app.close()
  })

  it('opens a pinned conversation by clicking its picture', async () => {
    const { renderer } = mount({ chats: { 'iMessage;+;chat240119384759': { pinned: true }, 'SMS;-;+14155550188': { pinned: true }, 'iMessage;+;chat881204957120': { pinned: true } } })
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    // A click lands on the element's centre, which for a pinned cell is the picture itself.
    await app.getByTestId('pinned-chat240119384759').click()
    await app.getByTestId('thread').getByText('Sunday lunch is at ours, 1pm. Bring the good bread.').waitFor({ timeoutMs: 10_000 })
    expect(await app.getByTestId('thread-title').textContent()).toBe('Family')

    await app.getByTestId('pinned-+14155550188').click()
    await app.getByTestId('thread').getByText('Perfect see you there').waitFor({ timeoutMs: 10_000 })
    expect(await app.getByTestId('thread-title').textContent()).toBe('Jordan Lee')

    await app.getByTestId('pinned-chat881204957120').click()
    await app.getByTestId('thread').getByText('Agree with Ben. Also the placeholder contrast is under 3:1.').waitFor({ timeoutMs: 10_000 })
    expect(await app.getByTestId('thread-title').textContent()).toBe('Design crit')

    await app.close()
  })

  it('starts a new conversation from the compose button', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('new-message').click()
    await app.getByTestId('to-field').fill('ben')
    await app.getByTestId('suggest-+14155550170').click()
    await app.getByTestId('new-chat-draft').fill('PR looks good')
    await app.getByTestId('new-chat-send').click()
    await app.getByTestId('thread').getByText('PR looks good').waitFor({ timeoutMs: 10_000 })
    expect(await app.getByTestId('thread-title').textContent()).toBe('Ben Okafor')

    await app.close()
  })

  it('masks the server password until it is revealed', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('settings').click()
    await app.getByTestId('server-password').waitFor({ timeoutMs: 10_000 })
    await app.getByTestId('server-password').fill('hunter2')
    let painted = renderer.getPaintedText()
    expect(painted).toContain('•••••••')
    expect(painted).not.toContain('hunter2')

    await app.getByTestId('server-password-reveal').click()
    for (let tries = 0; tries < 50 && !renderer.getPaintedText().includes('hunter2'); tries += 1) await new Promise((resolve) => setTimeout(resolve, 100))
    painted = renderer.getPaintedText()
    expect(painted).toContain('hunter2')

    await app.close()
  })

  it('jumps to the quoted message and opens the thread view', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })
    await app.getByTestId('thread').getByText('bring the charger this time 🔌').waitFor({ timeoutMs: 20_000 })

    // Demo guids are numbered per process, so find the quoted message from the tree.
    const nodes = await app.getByType('div').all()
    const quoted = nodes.map((node) => node.testId ?? '').find((id) => id.startsWith('quote-'))?.slice('quote-'.length)
    expect(quoted).toBeTruthy()
    await app.getByTestId(`quote-${quoted}`).click()
    await app.getByTestId(`replies-${quoted}`).waitFor({ timeoutMs: 10_000 })
    await app.getByTestId(`replies-${quoted}`).click()
    await app.getByTestId('thread-view').waitFor({ timeoutMs: 10_000 })
    const painted = renderer.getPaintedText()
    expect(painted).toContain('Thread')
    expect(painted).toContain('1 reply')
    expect(painted).toContain('bring the charger this time 🔌')

    await app.getByTestId('thread-back').click()
    await app.getByTestId('thread').waitFor({ timeoutMs: 10_000 })

    await app.close()
  })

  it('opens the details panel, then closes it again', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('info').click()
    await app.getByTestId('info-panel').waitFor({ timeoutMs: 10_000 })
    for (let tries = 0; tries < 50 && !renderer.getPaintedText().includes('Details'); tries += 1) await new Promise((resolve) => setTimeout(resolve, 100))
    expect(renderer.getPaintedText()).toContain('Details')

    await app.getByTestId('close-info').click()
    // The panel slides out before it leaves the tree.
    for (let tries = 0; tries < 50 && (await app.getByTestId('info-panel').all()).length > 0; tries += 1) await new Promise((resolve) => setTimeout(resolve, 100))
    expect((await app.getByTestId('info-panel').all()).length).toBe(0)

    await app.close()
  })

  it('shows the gallery in the details panel', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('composer').waitFor({ timeoutMs: 20_000 })

    // Nadia's thread is the one carrying photos.
    await app.getByTestId('chat-+34612345678').click()
    await app.getByTestId('info').click()
    await app.getByTestId('info-panel').waitFor({ timeoutMs: 10_000 })
    // Thumbnails wait for their square cut before painting, so the panel is up
    // before the pictures are.
    await app.getByTestId('gallery-photo-demo-att-3').waitFor({ timeoutMs: 10_000 })
    expect(renderer.getPaintedText()).toContain('Photos and files')

    await app.close()
  })

  it('replies to a message from its context menu', async () => {
    const { renderer } = mount()
    const app = await connectTest(renderer)
    await app.getByTestId('draft').waitFor({ timeoutMs: 20_000 })

    await app.getByTestId('thread').getByText('coffee at 4? the place on valencia').click({ button: 2 })
    await app.getByTestId('menu-reply').waitFor({ timeoutMs: 10_000 })
    await app.getByTestId('menu-reply').click()
    await app.getByTestId('reply-banner').waitFor({ timeoutMs: 10_000 })

    await app.getByTestId('draft').fill('on my way')
    await app.getByTestId('send').click()
    await app.getByTestId('thread').getByText('on my way').waitFor({ timeoutMs: 10_000 })
    for (let tries = 0; tries < 50 && (await app.getByTestId('reply-banner').all()).length > 0; tries += 1) await new Promise((resolve) => setTimeout(resolve, 100))
    expect((await app.getByTestId('reply-banner').all()).length).toBe(0)

    await app.close()
  })
})
