const PREAMBLE = `$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
`

/**
 * Runs a script in Windows PowerShell and resolves to its stdout, or null when
 * it failed. Values go in through the environment (read them back as
 * $env:NAME), so nothing a person typed is ever spliced into the script.
 * Windows PowerShell 5.1 rather than pwsh: it is on every install, it starts
 * in an STA thread (the clipboard and the file dialog both need one), and it
 * can load the WinRT toast types, which pwsh 7 cannot.
 */
export async function powershell(script: string, env: Record<string, string> = {}): Promise<string | null> {
  const proc = Bun.spawn(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', PREAMBLE + script], {
    env: { ...process.env, ...env },
    stdout: 'pipe',
    stderr: 'ignore',
    windowsHide: true,
  })
  const [out, code] = await Promise.all([new Response(proc.stdout).text(), proc.exited])
  return code === 0 ? out.trim() : null
}
