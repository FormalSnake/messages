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

function mount() {
  const { render, renderer } = createTestRoot({ width: 1120, height: 760 })
  const transport = new DemoTransport()
  render(<MessagesApp config={config} saveConfig={async () => undefined} transport={transport} />)
  return { renderer, transport }
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
})
