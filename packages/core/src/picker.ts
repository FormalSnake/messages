export interface PickFilesOptions {
  title?: string
  multiple?: boolean
}

/** Opens a native file picker and resolves to the chosen paths, or [] on cancel. */
export async function pickFiles(options: PickFilesOptions = {}): Promise<string[]> {
  if (Bun.which('gdbus')) return pickFilesLinux(options)
  if (Bun.which('osascript')) return pickFilesMac(options)
  throw new Error('No file picker available')
}

const RESPONSE_TIMEOUT_MS = 5 * 60_000

/**
 * The XDG desktop portal's FileChooser.OpenFile registers a request over
 * D-Bus and answers asynchronously with a Request.Response signal once the
 * user closes the dialog, so the result has to come from a `gdbus monitor`
 * running alongside the call rather than from the call's own (empty) return.
 */
async function pickFilesLinux(options: PickFilesOptions): Promise<string[]> {
  const title = options.title ?? 'Choose a file'
  const token = `messages${crypto.randomUUID().replace(/-/g, '')}`
  const monitor = Bun.spawn(['gdbus', 'monitor', '--session', '--dest', 'org.freedesktop.portal.Desktop'], {
    stdout: 'pipe',
    stderr: 'ignore',
  })

  try {
    const waitForResponse = readPortalResponse(monitor.stdout, token)

    const call = Bun.spawn(
      [
        'gdbus',
        'call',
        '--session',
        '--dest',
        'org.freedesktop.portal.Desktop',
        '--object-path',
        '/org/freedesktop/portal/desktop',
        '--method',
        'org.freedesktop.portal.FileChooser.OpenFile',
        '',
        title,
        `{'multiple': <${options.multiple ? 'true' : 'false'}>, 'handle_token': <'${token}'>}`,
      ],
      { stdout: 'ignore', stderr: 'ignore' },
    )
    const code = await call.exited
    if (code !== 0) return []

    const timeout = new Promise<string[]>(resolve => setTimeout(() => resolve([]), RESPONSE_TIMEOUT_MS))
    return await Promise.race([waitForResponse, timeout])
  } finally {
    monitor.kill()
  }
}

async function readPortalResponse(stdout: ReadableStream<Uint8Array>, token: string): Promise<string[]> {
  const reader = stdout.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) return []
      buffer += decoder.decode(value, { stream: true })
      let newline: number
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline)
        buffer = buffer.slice(newline + 1)
        if (line.includes(token) && line.includes('.Request.Response (')) return parsePortalResponse(line)
      }
    }
  } finally {
    reader.releaseLock()
  }
}

/**
 * Parses one `gdbus monitor` line for the FileChooser Request.Response
 * signal, e.g. `... .Request.Response (uint32 0, {'uris': <['file:///a b.jpg']>})`.
 * Response code 0 is success; 1 (cancelled) and 2 (ended another way) both
 * mean no files. Exported for tests.
 */
export function parsePortalResponse(line: string): string[] {
  const code = /\.Response \(uint32 (\d+)/.exec(line)?.[1]
  if (code !== '0') return []
  const uris = /'uris':\s*<\[(.*?)\]>/.exec(line)?.[1]
  if (!uris) return []
  return uris
    .split(',')
    .map(part => part.trim().replace(/^'|'$/g, ''))
    .filter(uri => uri.startsWith('file://'))
    .map(uri => decodeURIComponent(uri.slice('file://'.length)))
}

async function pickFilesMac(options: PickFilesOptions): Promise<string[]> {
  const title = escapeAppleScriptString(options.title ?? 'Choose a file')
  const multiClause = options.multiple ? 'with multiple selections allowed' : ''
  const script = `set thePick to choose file with prompt "${title}" ${multiClause}
set theList to {}
if class of thePick is list then
	set theList to thePick
else
	set theList to {thePick}
end if
set thePaths to {}
repeat with f in theList
	set end of thePaths to POSIX path of f
end repeat
set AppleScript's text item delimiters to linefeed
return thePaths as text`

  try {
    const proc = Bun.spawn(['osascript', '-e', script], { stdout: 'pipe', stderr: 'ignore' })
    const [out, code] = await Promise.all([new Response(proc.stdout).text(), proc.exited])
    if (code !== 0) return [] // user cancelled, or osascript raised an error
    return out
      .split('\n')
      .map(line => line.trim())
      .filter(Boolean)
  } catch {
    return []
  }
}

function escapeAppleScriptString(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
}
