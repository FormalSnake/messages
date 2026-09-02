/**
 * Launch the app on the demo fixtures and write a PNG.
 *
 *   bun run screenshot            writes screenshots/messages.png
 *   bun run screenshot out.png    writes that path instead
 */

import { mkdirSync } from 'node:fs'
import path from 'node:path'
import { launch } from '@gpuix/react/automation'

const out = process.argv[2] ?? 'screenshots/messages.png'
mkdirSync(path.dirname(out), { recursive: true })

const app = await launch({
  command: 'bun',
  args: ['app.tsx'],
  env: { GPUIX_BACKGROUND: '1', MESSAGES_DEMO: '1' },
})
await app.getByTestId('composer').waitFor({ timeoutMs: 60_000 })
await app.clock.pause()
await app.screenshot({ path: out })
await app.close()

console.log(`[screenshot] wrote ${out}`)
