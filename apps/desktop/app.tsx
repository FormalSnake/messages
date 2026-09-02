/**
 * Messages for Linux: the desktop window.
 *
 *   bun --hot app.tsx              connect to the server in ~/.config/messages/config.json
 *   MESSAGES_DEMO=1 bun --hot app.tsx   built-in fixtures, no Mac needed
 */

import { render } from '@gpuix/react'
import { ensureCacheDirs, loadConfig, saveConfig } from '@messages/core'
import { MessagesApp } from './src/ui/app'

const isEntryPoint = typeof Bun !== 'undefined' ? Bun.isStandaloneExecutable || Bun.main === import.meta.path : false

if (isEntryPoint) {
  const config = await loadConfig()
  await ensureCacheDirs()
  const darwin = process.platform === 'darwin'
  render(<MessagesApp config={config} saveConfig={saveConfig} />, {
    title: 'Messages',
    appName: 'Messages',
    width: 1120,
    height: 760,
    minWidth: 720,
    minHeight: 480,
    titlebarTransparent: darwin,
    trafficLightX: darwin ? 16 : undefined,
    trafficLightY: darwin ? 18 : undefined,
    focus: process.env.GPUIX_BACKGROUND !== '1',
  })
}
