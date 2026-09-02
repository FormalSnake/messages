import path from 'node:path'
import { attachmentsDir } from './config'

/** Saves an image from the clipboard into the attachment cache and returns its path, or null when the clipboard holds none. */
export async function clipboardImage(): Promise<string | null> {
  if (process.platform === 'darwin') return null
  const wayland = Boolean(process.env.WAYLAND_DISPLAY)
  try {
    if (wayland) {
      const types = await new Response(Bun.spawn(['wl-paste', '--list-types'], { stderr: 'ignore' }).stdout).text()
      const type = types.split('\n').find((line) => line.startsWith('image/'))
      if (!type) return null
      const ext = type.split('/')[1]?.replace('jpeg', 'jpg') ?? 'png'
      const target = path.join(attachmentsDir, `paste-${Date.now()}.${ext}`)
      const bytes = await new Response(Bun.spawn(['wl-paste', '--type', type], { stderr: 'ignore' }).stdout).arrayBuffer()
      if (bytes.byteLength === 0) return null
      await Bun.write(target, bytes)
      return target
    }
    const targets = await new Response(Bun.spawn(['xclip', '-selection', 'clipboard', '-t', 'TARGETS', '-o'], { stderr: 'ignore' }).stdout).text()
    const type = targets.split('\n').find((line) => line.startsWith('image/'))
    if (!type) return null
    const ext = type.split('/')[1]?.replace('jpeg', 'jpg') ?? 'png'
    const target = path.join(attachmentsDir, `paste-${Date.now()}.${ext}`)
    const bytes = await new Response(Bun.spawn(['xclip', '-selection', 'clipboard', '-t', type, '-o'], { stderr: 'ignore' }).stdout).arrayBuffer()
    if (bytes.byteLength === 0) return null
    await Bun.write(target, bytes)
    return target
  } catch {
    return null
  }
}

export async function copyText(text: string): Promise<void> {
  const command = process.platform === 'darwin' ? ['pbcopy'] : process.env.WAYLAND_DISPLAY ? ['wl-copy'] : ['xclip', '-selection', 'clipboard']
  try {
    const child = Bun.spawn(command, { stdin: 'pipe', stdout: 'ignore', stderr: 'ignore' })
    child.stdin.write(text)
    child.stdin.end()
    await child.exited
  } catch (error) {
    console.error(`clipboard: ${String(error)}`)
  }
}
