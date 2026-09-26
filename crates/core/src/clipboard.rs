//! Files and images through the clipboard. Plain text goes through GPUI's
//! own clipboard in the desktop crate.
//! Linux: wl-copy / wl-paste on Wayland, xclip on X11. macOS: osascript.
//! Windows: PowerShell.

use std::path::{Path, PathBuf};
use std::process::Stdio;

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use tokio::process::Command;

use crate::config::attachments_dir;

/// `text/uri-list` to existing local paths; comments and non-file URIs dropped.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| line.starts_with("file://"))
        .filter_map(|line| percent_encoding::percent_decode_str(&line["file://".len()..]).decode_utf8().ok().map(|s| PathBuf::from(s.into_owned())))
        .filter(|path| path.exists())
        .collect()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn command_output(cmd: &str, args: &[&str]) -> Vec<u8> {
    match Command::new(cmd).args(args).stdout(Stdio::piped()).stderr(Stdio::null()).output().await {
        Ok(out) => out.stdout,
        Err(_) => Vec::new(),
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn command_lines(cmd: &str, args: &[&str]) -> Vec<String> {
    let bytes = command_output(cmd, args).await;
    String::from_utf8_lossy(&bytes).lines().map(|line| line.trim().to_owned()).collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn save_clipboard_image(mime: &str, bytes: &[u8]) -> Vec<PathBuf> {
    if bytes.is_empty() {
        return vec![];
    }
    let ext = match mime.split('/').nth(1) {
        Some("jpeg") => "jpg".to_owned(),
        Some(other) => other.to_owned(),
        None => "png".to_owned(),
    };
    let dir = attachments_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let target = dir.join(format!("paste-{}.{ext}", now_ms()));
    match tokio::fs::write(&target, bytes).await {
        Ok(()) => vec![target],
        Err(_) => vec![],
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn linux_clipboard_attachments() -> Vec<PathBuf> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let types = if wayland { command_lines("wl-paste", &["--list-types"]).await } else { command_lines("xclip", &["-selection", "clipboard", "-t", "TARGETS", "-o"]).await };

    if types.iter().any(|t| t == "text/uri-list") {
        let bytes = if wayland {
            command_output("wl-paste", &["--type", "text/uri-list"]).await
        } else {
            command_output("xclip", &["-selection", "clipboard", "-t", "text/uri-list", "-o"]).await
        };
        let paths = parse_uri_list(&String::from_utf8_lossy(&bytes));
        if !paths.is_empty() {
            return paths;
        }
    }

    let Some(image_type) = types.into_iter().find(|t| t.starts_with("image/")) else { return vec![] };
    let bytes = if wayland { command_output("wl-paste", &["--type", &image_type]).await } else { command_output("xclip", &["-selection", "clipboard", "-t", &image_type, "-o"]).await };
    save_clipboard_image(&image_type, &bytes).await
}

#[cfg(target_os = "macos")]
async fn run_osascript(script: &str) -> Option<String> {
    let output = Command::new("osascript").arg("-e").arg(script).stdout(Stdio::piped()).stderr(Stdio::null()).output().await.ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
async fn mac_clipboard_attachments() -> Vec<PathBuf> {
    if let Some(file_path) = run_osascript("POSIX path of (the clipboard as \u{ab}class furl\u{bb})").await {
        if !file_path.is_empty() {
            return vec![PathBuf::from(file_path)];
        }
    }

    let dir = attachments_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let target = dir.join(format!("paste-{}.png", now_ms()));
    let script = format!(
        "set d to the clipboard as \u{ab}class PNGf\u{bb}\nset f to open for access POSIX file \"{}\" with write permission\nwrite d to f\nclose access f",
        target.display()
    );
    if run_osascript(&script).await.is_none() {
        return vec![];
    }
    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
        vec![target]
    } else {
        vec![]
    }
}

#[cfg(windows)]
async fn windows_clipboard_attachments() -> Vec<PathBuf> {
    let _ = tokio::fs::create_dir_all(attachments_dir()).await;
    let target = attachments_dir().join(format!("paste-{}.png", now_ms()));
    let script = "Add-Type -AssemblyName System.Windows.Forms, System.Drawing\n\
if ([Windows.Forms.Clipboard]::ContainsFileDropList()) { [Windows.Forms.Clipboard]::GetFileDropList() }\n\
elseif ([Windows.Forms.Clipboard]::ContainsImage()) {\n\
  [Windows.Forms.Clipboard]::GetImage().Save($env:MESSAGES_TARGET, [Drawing.Imaging.ImageFormat]::Png)\n\
  $env:MESSAGES_TARGET\n\
}";
    let env: std::collections::HashMap<&str, String> = [("MESSAGES_TARGET", target.to_string_lossy().into_owned())].into_iter().collect();
    let out = crate::windows::powershell(script, &env).await.unwrap_or_default();
    out.lines().map(str::trim).filter(|line| !line.is_empty()).map(PathBuf::from).filter(|path| path.exists()).collect()
}

/// Saves whatever the clipboard holds, files or an image, into the attachment cache and returns local paths. Empty when the clipboard holds only text.
pub async fn clipboard_attachments() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        windows_clipboard_attachments().await
    }
    #[cfg(target_os = "macos")]
    {
        mac_clipboard_attachments().await
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_clipboard_attachments().await
    }
}

/// Used where GPUI's clipboard is not reachable (the Windows toast path, tests).
pub async fn copy_text(text: &str) {
    #[cfg(windows)]
    {
        let env: std::collections::HashMap<&str, String> = [("MESSAGES_TEXT", text.to_owned())].into_iter().collect();
        if crate::windows::powershell("Set-Clipboard -Value $env:MESSAGES_TEXT", &env).await.is_none() {
            tracing::error!("clipboard: Set-Clipboard failed");
        }
        return;
    }
    #[cfg(not(windows))]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
            ("pbcopy", &[])
        } else if wayland {
            ("wl-copy", &[])
        } else {
            ("xclip", &["-selection", "clipboard"])
        };
        let mut child = match Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(child) => child,
            Err(error) => {
                tracing::error!("clipboard: {error}");
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            if let Err(error) = stdin.write_all(text.as_bytes()).await {
                tracing::error!("clipboard: {error}");
            }
            drop(stdin);
        }
        let _ = child.wait().await;
    }
}

/// A JPEG re-encoded once to `clip-<name>.png` beside the cache; PNG passes through; anything else is None.
pub async fn png_for(source: &Path, mime: &str) -> Option<PathBuf> {
    png_for_in(&attachments_dir(), source, mime).await
}

async fn png_for_in(dir: &Path, source: &Path, mime: &str) -> Option<PathBuf> {
    if mime == "image/png" {
        return Some(source.to_path_buf());
    }
    if mime != "image/jpeg" {
        return None;
    }
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let target = dir.join(format!("clip-{stem}.png"));
    if target.exists() {
        return Some(target);
    }
    let source_owned = source.to_path_buf();
    let target_owned = target.clone();
    let encoded = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let img = image::open(&source_owned)?;
        img.save(&target_owned)?;
        Ok(())
    })
    .await
    .ok()?;
    encoded.ok()?;
    Some(target)
}

/// encodeURI's safe set: unreserved plus the small set of reserved characters it leaves alone.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
const ENCODE_URI_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b';')
    .remove(b'/')
    .remove(b'?')
    .remove(b':')
    .remove(b'@')
    .remove(b'&')
    .remove(b'=')
    .remove(b'+')
    .remove(b'$')
    .remove(b',')
    .remove(b'#');

#[cfg(all(unix, not(target_os = "macos")))]
async fn linux_copy_file(source: &Path, mime: &str) {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let png = if mime.starts_with("image/") { png_for(source, mime).await } else { None };
    let mime_type = if png.is_some() { "image/png".to_owned() } else if mime.starts_with("image/") { mime.to_owned() } else { "text/uri-list".to_owned() };

    if mime_type == "text/uri-list" {
        let command = if wayland { vec!["wl-copy", "--type", &mime_type] } else { vec!["xclip", "-selection", "clipboard", "-t", &mime_type] };
        let mut child = match Command::new(command[0]).args(&command[1..]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(child) => child,
            Err(error) => {
                tracing::error!("clipboard: {error}");
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let encoded = utf8_percent_encode(&source.to_string_lossy(), ENCODE_URI_SAFE).to_string();
            let _ = stdin.write_all(format!("file://{encoded}\r\n").as_bytes()).await;
            drop(stdin);
        }
        let _ = child.wait().await;
        return;
    }

    let file_path = png.unwrap_or_else(|| source.to_path_buf());
    let file = match std::fs::File::open(&file_path) {
        Ok(file) => file,
        Err(error) => {
            tracing::error!("clipboard: {error}");
            return;
        }
    };
    let command = if wayland { vec!["wl-copy", "--type", &mime_type] } else { vec!["xclip", "-selection", "clipboard", "-t", &mime_type] };
    let _ = Command::new(command[0]).args(&command[1..]).stdin(Stdio::from(file)).stdout(Stdio::null()).stderr(Stdio::null()).status().await;
}

/// An image goes on as the bitmap plus the file, so a browser pastes the picture and Finder pastes the file.
#[cfg(target_os = "macos")]
async fn mac_copy_file(source: &Path, mime: &str) {
    let image_line = if mime.starts_with("image/") {
        format!(
            "const image = $.NSImage.alloc.initWithContentsOfFile({0})\nif (!image.isNil()) items.addObject(image)",
            serde_json::to_string(&source.to_string_lossy()).unwrap_or_default()
        )
    } else {
        String::new()
    };
    let script = format!(
        "ObjC.import('AppKit')\nconst pb = $.NSPasteboard.generalPasteboard\npb.clearContents\nconst items = $.NSMutableArray.alloc.init\n{image_line}\nitems.addObject($.NSURL.fileURLWithPath({0}))\npb.writeObjects(items)\n",
        serde_json::to_string(&source.to_string_lossy()).unwrap_or_default()
    );
    let _ = Command::new("osascript").arg("-l").arg("JavaScript").arg("-e").arg(&script).stdout(Stdio::null()).stderr(Stdio::null()).status().await;
}

/// Same pairing as macOS: the bitmap for whatever pastes pictures, the file drop for Explorer.
#[cfg(windows)]
async fn windows_copy_file(source: &Path, mime: &str) {
    let script = "Add-Type -AssemblyName System.Windows.Forms, System.Drawing\n\
$data = New-Object Windows.Forms.DataObject\n\
if ($env:MESSAGES_IMAGE -eq '1') { try { $data.SetImage([Drawing.Image]::FromFile($env:MESSAGES_SOURCE)) } catch {} }\n\
$files = New-Object Collections.Specialized.StringCollection\n\
[void]$files.Add($env:MESSAGES_SOURCE)\n\
$data.SetFileDropList($files)\n\
[Windows.Forms.Clipboard]::SetDataObject($data, $true)";
    let env: std::collections::HashMap<&str, String> =
        [("MESSAGES_SOURCE", source.to_string_lossy().into_owned()), ("MESSAGES_IMAGE", if mime.starts_with("image/") { "1".to_owned() } else { "0".to_owned() })].into_iter().collect();
    let _ = crate::windows::powershell(script, &env).await;
}

/// Puts a file from the attachment cache on the clipboard: an image as an image, anything else as a file.
pub async fn copy_file(source: &Path, mime: &str) {
    #[cfg(target_os = "macos")]
    {
        mac_copy_file(source, mime).await;
    }
    #[cfg(windows)]
    {
        windows_copy_file(source, mime).await;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_copy_file(source, mime).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This test file itself, guaranteed to exist and stable regardless of the test runner's cwd.
    fn existing_file() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/clipboard.rs")
    }

    #[test]
    fn decodes_file_uri_entries_that_exist() {
        let full_path = existing_file();
        let uri = format!("file://{}", utf8_percent_encode(&full_path.to_string_lossy(), ENCODE_URI_SAFE));
        let text = format!("{uri}\n");
        assert_eq!(parse_uri_list(&text), vec![full_path]);
    }

    #[test]
    fn drops_entries_that_are_not_file_uris() {
        assert!(parse_uri_list("https://example.com/a.jpg\n").is_empty());
    }

    #[test]
    fn drops_comments_and_blank_lines() {
        let existing = existing_file();
        let uri = format!("file://{}", utf8_percent_encode(&existing.to_string_lossy(), ENCODE_URI_SAFE));
        let text = format!("# a comment\n\n{uri}\n");
        assert_eq!(parse_uri_list(&text), vec![existing]);
    }

    #[tokio::test]
    async fn hands_a_png_straight_back_and_leaves_formats_it_cannot_decode_alone() {
        let dir = crate::testing::temp_dir("clip");
        let png = dir.join("a.png");
        assert_eq!(png_for_in(&dir, &png, "image/png").await, Some(png));
        assert_eq!(png_for_in(&dir, &dir.join("a.heic"), "image/heic").await, None);
        assert_eq!(png_for_in(&dir, &dir.join("a.gif"), "image/gif").await, None);
    }

    #[tokio::test]
    async fn re_encodes_a_jpeg_as_png_once() {
        let dir = crate::testing::temp_dir("clip");
        let jpeg = dir.join("photo.jpg");
        image::RgbImage::from_pixel(4, 3, image::Rgb([200, 10, 10])).save(&jpeg).unwrap();
        let png = png_for_in(&dir, &jpeg, "image/jpeg").await.unwrap();
        assert_eq!(png, dir.join("clip-photo.png"));
        assert_eq!(image::image_dimensions(&png).unwrap(), (4, 3));
        assert_eq!(&std::fs::read(&png).unwrap()[..4], b"\x89PNG");
        std::fs::write(&png, b"kept").unwrap();
        assert_eq!(png_for_in(&dir, &jpeg, "image/jpeg").await, Some(png.clone()));
        assert_eq!(std::fs::read(&png).unwrap(), b"kept");
    }

    #[test]
    fn drops_file_uri_entries_whose_path_does_not_exist() {
        assert!(parse_uri_list("file:///no/such/file-xyz.jpg\n").is_empty());
    }
}
