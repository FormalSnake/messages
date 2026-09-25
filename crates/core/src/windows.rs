//! Port of packages/core/src/windows.ts.

use std::collections::HashMap;

/// Runs Windows PowerShell 5.1 (STA thread, WinRT toast types) and returns stdout, or None when it failed.
/// Values go in through the environment, never spliced into the script.
pub async fn powershell(script: &str, env: &HashMap<&str, String>) -> Option<String> {
    let _ = (script, env);
    unimplemented!()
}
