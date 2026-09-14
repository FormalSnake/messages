import { existsSync } from 'node:fs'
import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { decode as decodeJpeg } from 'jpeg-js'
import { PNG } from 'pngjs'
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

/** Everything Linux apps paste as an image comes in as PNG, so a JPEG is re-encoded once and kept beside the cache. */
export async function pngFor(source: string, mime: string): Promise<string | null> {
  if (mime === 'image/png') return source
  if (mime !== 'image/jpeg') return null
  const target = path.join(attachmentsDir, `clip-${path.basename(source, path.extname(source))}.png`)
  if (existsSync(target)) return target
  const decoded = decodeJpeg(await readFile(source), { useTArray: true, maxMemoryUsageInMB: 1024 })
  const png = new PNG({ width: decoded.width, height: decoded.height })
  png.data = Buffer.from(decoded.data.buffer, decoded.data.byteOffset, decoded.data.byteLength)
  await writeFile(target, PNG.sync.write(png))
  return target
}

async function linuxCopyFile(source: string, mime: string): Promise<void> {
  const wayland = Boolean(process.env.WAYLAND_DISPLAY)
  const png = mime.startsWith('image/') ? await pngFor(source, mime) : null
  const type = png ? 'image/png' : mime.startsWith('image/') ? mime : 'text/uri-list'
  const command = wayland ? ['wl-copy', '--type', type] : ['xclip', '-selection', 'clipboard', '-t', type]
  if (type === 'text/uri-list') {
    const child = Bun.spawn(command, { stdin: 'pipe', stdout: 'ignore', stderr: 'ignore' })
    child.stdin.write(`file://${encodeURI(source)}\r\n`)
    child.stdin.end()
    await child.exited
    return
  }
  await Bun.spawn(command, { stdin: Bun.file(png ?? source), stdout: 'ignore', stderr: 'ignore' }).exited
}

/** An image goes on as the bitmap plus the file, so a browser pastes the picture and Finder pastes the file. */
async function macCopyFile(source: string, mime: string): Promise<void> {
  const script = `
ObjC.import('AppKit')
const pb = $.NSPasteboard.generalPasteboard
pb.clearContents
const items = $.NSMutableArray.alloc.init
${mime.startsWith('image/') ? `const image = $.NSImage.alloc.initWithContentsOfFile(${JSON.stringify(source)})\nif (!image.isNil()) items.addObject(image)` : ''}
items.addObject($.NSURL.fileURLWithPath(${JSON.stringify(source)}))
pb.writeObjects(items)
`
  const child = Bun.spawn(['osascript', '-l', 'JavaScript', '-e', script], { stdout: 'ignore', stderr: 'ignore' })
  await child.exited
}

/** Puts a file from the attachment cache on the clipboard: an image as an image, anything else as a file. */
export async function copyFile(source: string, mime = ''): Promise<void> {
  try {
    if (process.platform === 'darwin') await macCopyFile(source, mime)
    else await linuxCopyFile(source, mime)
  } catch (error) {
    console.error(`clipboard: ${String(error)}`)
  }
}
