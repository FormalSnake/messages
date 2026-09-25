//! Port of packages/core/src/windows.ts.

use std::collections::HashMap;
use std::process::Stdio;

use tokio::process::Command;

const PREAMBLE: &str = "$ErrorActionPreference = 'Stop'\n[Console]::OutputEncoding = [Text.Encoding]::UTF8\n";

/// Hides the console window PowerShell would otherwise flash open.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Runs a script in Windows PowerShell and resolves to its stdout, or None when
/// it failed. Values go in through the environment (read back as `$env:NAME`),
/// so nothing a person typed is ever spliced into the script. Windows
/// PowerShell 5.1 rather than pwsh: it is on every install, it starts in an
/// STA thread (the clipboard and the file dialog both need one), and it can
/// load the WinRT toast types, which pwsh 7 cannot.
pub async fn powershell(script: &str, env: &HashMap<&str, String>) -> Option<String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &format!("{PREAMBLE}{script}")]);
    cmd.envs(env.iter().map(|(key, value)| (*key, value.clone())));
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
