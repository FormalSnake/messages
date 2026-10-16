//! Small pieces every screen shares: avatars from the core's people, image sources for cached
//! files and data URLs, focus by element id, shortcut labels.

use std::path::Path;

use messages_core::{Chat, Handle, chat_title, handle_name};
use nd::prelude::*;

/// What `image()` shows for a cached file path or a data URL (contact photos, fixtures).
pub fn image_source(src: &str) -> Option<ImageSource> {
    if let Some(rest) = src.strip_prefix("data:") {
        let (meta, payload) = rest.split_once(',')?;
        use base64::Engine as _;
        let bytes = if meta.ends_with(";base64") {
            base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()?
        } else {
            payload.as_bytes().to_vec()
        };
        return Some(ImageSource::bytes(format!("data:{:016x}", fnv(src.as_bytes())), bytes));
    }
    Some(ImageSource::path(Path::new(src)))
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

pub fn person(handle: &Handle) -> Person {
    let person = Person::new(handle_name(handle).to_owned());
    match handle.avatar.as_deref().and_then(image_source) {
        Some(source) => person.image(source),
        None => person,
    }
}

pub fn avatar_of(handle: &Handle, size: f32) -> Avatar {
    avatar(person(handle), size)
}

/// A chat's picture: the group photo, the people clustered for a group without one, or the
/// person of a one-to-one chat.
pub fn chat_avatar(chat: &Chat, size: f32) -> Avatar {
    if chat.is_group {
        if let Some(source) = chat.icon.as_deref().and_then(image_source) {
            return avatar(Person::new(chat_title(chat)).image(source), size);
        }
        return avatar_group(chat.participants.iter().take(4).map(person), size);
    }
    match chat.participants.first() {
        Some(handle) => avatar(person(handle), size),
        None => avatar(Person::new(chat_title(chat)), size),
    }
}

/// Focuses the element whose id is `id` in `window`. nd focuses by node, and an app holds no
/// node ids, so the semantic tree is searched for it.
pub fn focus_id(window: &Window, cx: &mut App, id: &str) -> bool {
    let window = window.id();
    let Some(tree) = cx.semantic_window(window) else { return false };
    let Some(node) = tree.root.iter().find(|n| n.test_id.as_deref() == Some(id)).map(|n| n.id) else {
        return false;
    };
    cx.focus(window, node)
}

/// Bubbles `action` from the window's focused element, as a menu item or key binding would.
pub fn dispatch(window: &Window, cx: &mut App, action: impl Action) {
    cx.dispatch_action(window.id(), &action);
}

pub fn dispatch_boxed(window: &Window, cx: &mut App, action: Box<dyn Action>) {
    cx.dispatch_action(window.id(), action.as_ref());
}

/// "⇧⌘U" on macOS, "Ctrl+Shift+U" elsewhere.
pub fn shortcut(key: &str, shift: bool) -> SharedString {
    if cfg!(target_os = "macos") {
        format!("{}\u{2318}{key}", if shift { "\u{21e7}" } else { "" }).into()
    } else if shift {
        format!("Ctrl+Shift+{key}").into()
    } else {
        format!("Ctrl+{key}").into()
    }
}

/// A small round dot: unread markers, connection status.
pub fn dot(color: Hsla, size: f32) -> Div {
    div().size(px(size)).flex_none().rounded_full().bg(color)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// A preview on one line (newlines and runs of spaces folded), cut at `max` characters on a
/// word boundary.
pub fn clamp_preview(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max).collect();
    if let Some(space) = cut.rfind(' ').filter(|i| *i > max / 2) {
        cut.truncate(space);
    }
    cut.push('\u{2026}');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_cut_at_a_word() {
        assert_eq!(clamp_preview("short", 10), "short");
        assert_eq!(clamp_preview("one two three four five", 14), "one two three\u{2026}");
        assert_eq!(clamp_preview("a\nb", 10), "a b");
    }

    #[test]
    fn data_urls_become_bytes() {
        let source = image_source("data:image/png;base64,aGVsbG8=").expect("decodes");
        match source {
            ImageSource::Bytes { bytes, .. } => assert_eq!(&*bytes, b"hello"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(image_source("/tmp/a.png"), Some(ImageSource::Path(_))));
    }
}
