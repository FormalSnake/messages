import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { configDir } from '@messages/core'
import { applyPalette, resetPalette } from './theme'

/** A flat `{ token: "#rrggbb" }` object; matugen renders its template here. Keys are the palette tokens in `theme.ts`. */
export const themeFile = path.join(configDir, 'theme.json')

const POLL_MS = 1000

/**
 * Follows the theme file while the app runs: matugen rewrites it on every
 * wallpaper or mode change, and polling it is what the user's other apps do
 * too. `onChange` fires after the palette has been swapped in.
 */
export function watchTheme(onChange: () => void): () => void {
  let last: string | null = null
  const check = async () => {
    let text: string
    try {
      text = await readFile(themeFile, 'utf8')
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') console.error(`theme: ${themeFile}: ${String(error)}`)
      if (last === null) return
      last = null
      resetPalette()
      onChange()
      return
    }
    if (text === last) return
    last = text
    try {
      applyPalette(JSON.parse(text) as Record<string, unknown>)
      onChange()
    } catch (error) {
      console.error(`theme: ${themeFile}: ${String(error)}`)
    }
  }
  void check()
  const timer = setInterval(() => void check(), POLL_MS)
  return () => clearInterval(timer)
}
