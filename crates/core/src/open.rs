//! Port of packages/core/src/open.ts: URLs and files to the desktop, link
//! splitting, and voice note playback through an external player.

use std::path::Path;

/// xdg-open on Linux, open on macOS, url.dll FileProtocolHandler on Windows. Never blocks.
pub fn open_external(target: &str) {
    let _ = target;
    unimplemented!()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSegment {
    Text(String),
    Link { text: String, href: String },
}

/// http(s) and www. links, trailing punctuation trimmed, www. gets https://.
pub fn split_links(text: &str) -> Vec<TextSegment> {
    let _ = text;
    unimplemented!()
}

/// Stops the previous clip, then plays through afplay (macOS) or mpv, ffplay, paplay (Linux). False when none exists.
pub fn play_audio(path: &Path) -> bool {
    let _ = path;
    unimplemented!()
}

pub fn stop_audio() {
    unimplemented!()
}

pub fn is_audio_playing() -> bool {
    unimplemented!()
}
