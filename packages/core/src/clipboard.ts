import { existsSync } from 'node:fs'
import path from 'node:path'
import { attachmentsDir } from './config'

/**
 * Parses a `text/uri-list` payload into local file paths, decoding
 * percent-escapes and dropping anything that isn't a `file://` URI pointing
 * at a path that actually exists. Exported for tests.
 */
export function parseUriList(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(line => line && !line.startsWith('#'))
    .filter(line => line.startsWith('file://'))
    .map(line => decodeURIComponent(line.slice('file://'.length)))
    .filter(p => existsSync(p))
}

async function pasteTypes(command: string[]): Promise<string[]> {
  const out = await new Response(Bun.spawn(command, { stderr: 'ignore' }).stdout).text()
  return out.split('\n').map(line => line.trim())
}

async function pasteBytes(command: string[]): Promise<ArrayBuffer> {
  return new Response(Bun.spawn(command, { stderr: 'ignore' }).stdout).arrayBuffer()
}

async function pasteText(command: string[]): Promise<string> {
  return new Response(Bun.spawn(command, { stderr: 'ignore' }).stdout).text()
}

async function saveClipboardImage(mime: string, bytes: ArrayBuffer): Promise<string[]> {
  if (bytes.byteLength === 0) return []
  const ext = mime.split('/')[1]?.replace('jpeg', 'jpg') ?? 'png'
  const target = path.join(attachmentsDir, `paste-${Date.now()}.${ext}`)
  await Bun.write(target, bytes)
  return [target]
}

async function linuxClipboardAttachments(): Promise<string[]> {
  const wayland = Boolean(process.env.WAYLAND_DISPLAY)
  const types = await pasteTypes(
    wayland ? ['wl-paste', '--list-types'] : ['xclip', '-selection', 'clipboard', '-t', 'TARGETS', '-o'],
  )

  if (types.includes('text/uri-list')) {
    const text = await pasteText(
      wayland
        ? ['wl-paste', '--type', 'text/uri-list']
        : ['xclip', '-selection', 'clipboard', '-t', 'text/uri-list', '-o'],
    )
    const paths = parseUriList(text)
    if (paths.length) return paths
  }

  const imageType = types.find(type => type.startsWith('image/'))
  if (!imageType) return []
  const bytes = await pasteBytes(
    wayland ? ['wl-paste', '--type', imageType] : ['xclip', '-selection', 'clipboard', '-t', imageType, '-o'],
  )
  return saveClipboardImage(imageType, bytes)
}

async function runOsascript(script: string): Promise<string | null> {
  const proc = Bun.spawn(['osascript', '-e', script], { stdout: 'pipe', stderr: 'ignore' })
  const [out, code] = await Promise.all([new Response(proc.stdout).text(), proc.exited])
  return code === 0 ? out.trim() : null
}

async function macClipboardAttachments(): Promise<string[]> {
  const filePath = await runOsascript('POSIX path of (the clipboard as «class furl»)')
  if (filePath) return [filePath]

  const target = path.join(attachmentsDir, `paste-${Date.now()}.png`)
  const wrote = await runOsascript(
    `set d to the clipboard as «class PNGf»\nset f to open for access POSIX file "${target}" with write permission\nwrite d to f\nclose access f`,
  )
  if (wrote === null) return []
  return (await Bun.file(target).exists()) ? [target] : []
}

/** Saves whatever the clipboard holds, files or an image, into the attachment cache and returns local paths. Empty when the clipboard holds only text. */
export async function clipboardAttachments(): Promise<string[]> {
  try {
    return process.platform === 'darwin' ? await macClipboardAttachments() : await linuxClipboardAttachments()
  } catch {
    return []
  }
}

/** @deprecated use clipboardAttachments() */
export async function clipboardImage(): Promise<string | null> {
  const [first] = await clipboardAttachments()
  return first ?? null
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
