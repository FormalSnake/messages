//! Port of packages/core/src/open.ts: URLs and files to the desktop, link
//! splitting, and voice note playback through an external player.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;

/// xdg-open on Linux, open on macOS, url.dll FileProtocolHandler on Windows (`cmd /c start` would split a URL at its first `&`). Never blocks.
pub fn open_external(target: &str) {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(target).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    } else if cfg!(windows) {
        Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(target).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    } else {
        Command::new("xdg-open").arg(target).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    };
    match result {
        // Waited on a thread of its own, or every link opened leaves a zombie until the app exits.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(error) => tracing::error!("open: {error}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSegment {
    Text(String),
    Link { text: String, href: String },
}

fn link_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"(?i)https?://[^\s<>"'）)]+|www\.[^\s<>"'）)]+"#).unwrap())
}

/// http(s) and www. links, trailing punctuation trimmed, www. gets https://.
pub fn split_links(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut last = 0usize;
    for m in link_regex().find_iter(text) {
        let start = m.start();
        let trimmed = m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if start > last {
            segments.push(TextSegment::Text(text[last..start].to_owned()));
        }
        let href = if trimmed.starts_with("http") { trimmed.to_owned() } else { format!("https://{trimmed}") };
        segments.push(TextSegment::Link { text: trimmed.to_owned(), href });
        last = start + trimmed.len();
    }
    if last < text.len() {
        segments.push(TextSegment::Text(text[last..].to_owned()));
    }
    segments
}

fn player() -> &'static Mutex<Option<Child>> {
    static PLAYER: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    PLAYER.get_or_init(|| Mutex::new(None))
}

/// Polls the child on its own OS thread (never the tokio runtime) so `is_audio_playing` clears itself once playback ends.
fn watch_player() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(200));
        let mut guard = player().lock();
        match guard.as_mut() {
            Some(child) => {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    *guard = None;
                    return;
                }
            }
            None => return,
        }
    });
}

/// Stops the previous clip, then plays through afplay (macOS) or mpv, ffplay, paplay (Linux). False when none exists.
pub fn play_audio(path: &Path) -> bool {
    stop_audio();
    let candidates: &[&[&str]] =
        if cfg!(target_os = "macos") { &[&["afplay"]] } else { &[&["mpv", "--no-video", "--really-quiet"], &["ffplay", "-nodisp", "-autoexit", "-loglevel", "quiet"], &["paplay"]] };
    for command in candidates {
        let bin = command[0];
        if which::which(bin).is_err() {
            continue;
        }
        let mut cmd = Command::new(bin);
        cmd.args(&command[1..]).arg(path).stdout(Stdio::null()).stderr(Stdio::null());
        match cmd.spawn() {
            Ok(child) => {
                *player().lock() = Some(child);
                watch_player();
                return true;
            }
            Err(error) => tracing::error!("audio: {error}"),
        }
    }
    false
}

pub fn stop_audio() {
    if let Some(mut child) = player().lock().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn is_audio_playing() -> bool {
    player().lock().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_plain_text_with_no_links() {
        assert_eq!(split_links("just words"), vec![TextSegment::Text("just words".to_owned())]);
    }

    #[test]
    fn links_a_bare_url_with_surrounding_text() {
        let segments = split_links("see https://example.com/a for details");
        assert_eq!(
            segments,
            vec![
                TextSegment::Text("see ".to_owned()),
                TextSegment::Link { text: "https://example.com/a".to_owned(), href: "https://example.com/a".to_owned() },
                TextSegment::Text(" for details".to_owned()),
            ]
        );
    }

    #[test]
    fn trims_trailing_punctuation_from_a_link_leaving_it_as_trailing_text() {
        let segments = split_links("see https://example.org/x.");
        assert_eq!(
            segments,
            vec![
                TextSegment::Text("see ".to_owned()),
                TextSegment::Link { text: "https://example.org/x".to_owned(), href: "https://example.org/x".to_owned() },
                TextSegment::Text(".".to_owned()),
            ]
        );
    }

    #[test]
    fn prefixes_a_www_link_with_https() {
        let segments = split_links("www.example.com");
        assert_eq!(segments, vec![TextSegment::Link { text: "www.example.com".to_owned(), href: "https://www.example.com".to_owned() }]);
    }
}
