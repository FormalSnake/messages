//! Colour emoji on Linux. GPUI's cosmic-text backend only paints a glyph in
//! colour when its face's PostScript name is `NotoColorEmoji`, drops any face
//! with no `m` glyph when a style names its family, and falls back through a
//! hardcoded list that puts Noto Sans Symbols 2 and DejaVu Sans ahead of any
//! emoji font, so U+2764 and U+1F44D come out as monochrome outlines.
//!
//! The fix registers the system's colour emoji font a second time under
//! `FAMILY`, with a cmap that also maps `m` and the PostScript name GPUI
//! wants, and styles emoji runs with that family. Only a small header is
//! rewritten: it is mapped in front of the untouched font file, whose pages
//! stay file-backed, so a 116 MB Apple Color Emoji costs no heap.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::*;

pub const FAMILY: &str = "Messages Emoji";
const POSTSCRIPT: &str = "NotoColorEmoji";

static LOADED: AtomicBool = AtomicBool::new(false);

/// True once the renamed face is registered with the text system.
pub fn loaded() -> bool {
    LOADED.load(Ordering::Relaxed)
}

/// `text` with `highlights`, every emoji cluster set in `FAMILY` so it paints
/// in colour. Plain `StyledText` when the family is not loaded (macOS, or no
/// colour font found) or the text has no emoji.
pub fn styled_text(text: SharedString, highlights: Vec<(Range<usize>, HighlightStyle)>, color: Hsla) -> StyledText {
    let emoji = if loaded() { emoji_ranges(&text) } else { Vec::new() };
    if emoji.is_empty() {
        return StyledText::new(text).with_highlights(highlights);
    }
    let base = TextStyle { color, font_family: crate::theme::font_sans(), ..TextStyle::default() };
    let mut cuts: Vec<usize> = vec![0, text.len()];
    for range in highlights.iter().map(|(range, _)| range).chain(&emoji) {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let runs = cuts
        .windows(2)
        .map(|pair| {
            let (start, end) = (pair[0], pair[1]);
            let style = highlights.iter().filter(|(range, _)| range.start <= start && end <= range.end).fold(base.clone(), |style, (_, highlight)| style.highlight(*highlight));
            let mut run = style.to_run(end - start);
            if emoji.iter().any(|range| range.start <= start && end <= range.end) {
                run.font.family = FAMILY.into();
            }
            run
        })
        .collect();
    StyledText::new(text).with_runs(runs)
}

/// Byte ranges of the emoji grapheme clusters in `text`.
pub fn emoji_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let cluster = messages_core::format::first_grapheme(&text[at..]);
        if cluster.is_empty() {
            break;
        }
        if !cluster.is_ascii() && messages_core::format::is_emoji_only(cluster, 1) {
            match ranges.last_mut() {
                Some(last) if last.end == at => last.end = at + cluster.len(),
                _ => ranges.push(at..at + cluster.len()),
            }
        }
        at += cluster.len();
    }
    ranges
}

/// Finds and registers the colour emoji font off the foreground thread, then
/// repaints so emoji already on screen pick it up.
pub fn install(cx: &mut App) {
    #[cfg(target_os = "linux")]
    {
        let task = cx.background_spawn(async { linux::load() });
        cx.spawn(async move |cx| {
            let Some(bytes) = task.await else {
                eprintln!("messages: no colour emoji font registered; emoji use the text fallback");
                return;
            };
            cx.update(|cx| {
                if cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(bytes)]).is_ok() {
                    LOADED.store(true, Ordering::Relaxed);
                    crate::trace::log_if_enabled("colour emoji font registered");
                    cx.refresh_windows();
                }
            });
        })
        .detach();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cx;
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::File;
    use std::os::fd::AsRawFd as _;
    use std::process::Command;

    /// The first colour font fontconfig knows that covers U+1F600.
    fn system_emoji_font() -> Option<String> {
        let output = Command::new("fc-list").args([":color=true:charset=1f600", "file"]).output().ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        text.lines().map(|line| line.trim().trim_end_matches(':').to_owned()).find(|path| path.ends_with(".ttf") || path.ends_with(".otf"))
    }

    pub fn load() -> Option<&'static [u8]> {
        let path = system_emoji_font()?;
        let file = File::open(&path).ok()?;
        let len = usize::try_from(file.metadata().ok()?.len()).ok()?;
        // The directory, cmap and name are all near the start; the table
        // records say where, so reading a bounded head is enough to patch them.
        let head = super::read_tables(&file, len)?;
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        let prefix = super::prefix(&head, page)?;
        // SAFETY: the anonymous mapping is sized for prefix + file and the
        // file is mapped over its tail with MAP_FIXED, so the slice covers
        // exactly two live mappings that are never unmapped (leaked for the
        // process, like any registered font).
        unsafe {
            let total = prefix.len() + len;
            let base = libc::mmap(std::ptr::null_mut(), total, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0);
            if base == libc::MAP_FAILED {
                return None;
            }
            std::ptr::copy_nonoverlapping(prefix.as_ptr(), base.cast::<u8>(), prefix.len());
            let tail = libc::mmap(base.cast::<u8>().add(prefix.len()).cast(), len, libc::PROT_READ, libc::MAP_PRIVATE | libc::MAP_FIXED, file.as_raw_fd(), 0);
            if tail == libc::MAP_FAILED {
                libc::munmap(base, total);
                return None;
            }
            libc::mprotect(base, prefix.len(), libc::PROT_READ);
            Some(std::slice::from_raw_parts(base.cast::<u8>(), total))
        }
    }
}

/// The font's bytes as far as the end of its `cmap` and `name` tables, which
/// is all `prefix` reads.
#[cfg(target_os = "linux")]
fn read_tables(file: &std::fs::File, len: usize) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let mut header = vec![0u8; 12];
    let mut reader = file;
    reader.read_exact(&mut header).ok()?;
    let count = usize::from(u16_at(&header, 4)?);
    let mut directory = vec![0u8; 16 * count];
    reader.read_exact(&mut directory).ok()?;
    header.extend_from_slice(&directory);
    let end = tables(&header)?
        .iter()
        .filter(|table| &table.tag == b"cmap" || &table.tag == b"name")
        .map(|table| table.offset as usize + table.length as usize)
        .max()?;
    if end > len {
        return None;
    }
    let mut head = vec![0u8; end];
    std::os::unix::fs::FileExt::read_exact_at(file, &mut head, 0).ok()?;
    Some(head)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[derive(Clone, Copy)]
struct Table {
    tag: [u8; 4],
    checksum: u32,
    offset: u32,
    length: u32,
}

fn tables(font: &[u8]) -> Option<Vec<Table>> {
    let version = u32_at(font, 0)?;
    // TrueType, 'true' or 'OTTO'; a collection (.ttc) is left alone.
    if ![0x0001_0000, 0x7472_7565, 0x4F54_544F].contains(&version) {
        return None;
    }
    let count = usize::from(u16_at(font, 4)?);
    (0..count)
        .map(|index| {
            let at = 12 + 16 * index;
            Some(Table { tag: font.get(at..at + 4)?.try_into().ok()?, checksum: u32_at(font, at + 4)?, offset: u32_at(font, at + 8)?, length: u32_at(font, at + 12)? })
        })
        .collect()
}

fn table<'a>(font: &'a [u8], all: &[Table], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let entry = all.iter().find(|table| &table.tag == tag)?;
    font.get(entry.offset as usize..entry.offset as usize + entry.length as usize)
}

/// (first char, last char, first glyph) runs of the font's Unicode cmap.
fn cmap_groups(cmap: &[u8]) -> Option<Vec<(u32, u32, u32)>> {
    let count = usize::from(u16_at(cmap, 2)?);
    let records: Vec<(u16, u16, usize)> = (0..count).filter_map(|index| Some((u16_at(cmap, 4 + 8 * index)?, u16_at(cmap, 6 + 8 * index)?, u32_at(cmap, 8 + 8 * index)? as usize))).collect();
    let unicode = |platform: u16, encoding: u16| platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10));
    let format_of = |offset: usize| u16_at(cmap, offset);
    if let Some(&(_, _, offset)) = records.iter().find(|&&(platform, encoding, offset)| unicode(platform, encoding) && format_of(offset) == Some(12)) {
        let groups = u32_at(cmap, offset + 12)? as usize;
        return (0..groups).map(|index| Some((u32_at(cmap, offset + 16 + 12 * index)?, u32_at(cmap, offset + 20 + 12 * index)?, u32_at(cmap, offset + 24 + 12 * index)?))).collect();
    }
    let &(_, _, offset) = records.iter().find(|&&(platform, encoding, offset)| unicode(platform, encoding) && format_of(offset) == Some(4))?;
    let segments = usize::from(u16_at(cmap, offset + 6)? / 2);
    let (ends, starts, deltas, ranges) = (offset + 14, offset + 16 + 2 * segments, offset + 16 + 4 * segments, offset + 16 + 6 * segments);
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for segment in 0..segments {
        let (end, start) = (u32::from(u16_at(cmap, ends + 2 * segment)?), u32::from(u16_at(cmap, starts + 2 * segment)?));
        let delta = u32::from(u16_at(cmap, deltas + 2 * segment)?);
        let range_at = ranges + 2 * segment;
        let range = usize::from(u16_at(cmap, range_at)?);
        if start > end || start == 0xFFFF {
            continue;
        }
        for char in start..=end {
            let glyph = if range == 0 {
                (char + delta) & 0xFFFF
            } else {
                match u16_at(cmap, range_at + range + 2 * (char - start) as usize)? {
                    0 => 0,
                    glyph => (u32::from(glyph) + delta) & 0xFFFF,
                }
            };
            if glyph == 0 {
                continue;
            }
            match groups.last_mut() {
                Some(last) if last.1 + 1 == char && last.2 + (last.1 - last.0) + 1 == glyph => last.1 = char,
                _ => groups.push((char, char, glyph)),
            }
        }
    }
    Some(groups)
}

fn glyph_for(groups: &[(u32, u32, u32)], char: u32) -> Option<u32> {
    groups.iter().find(|group| group.0 <= char && char <= group.1).map(|group| group.2 + char - group.0)
}

/// A format 12 cmap with every original mapping plus `m`.
fn patched_cmap(original: &[u8]) -> Option<Vec<u8>> {
    let mut groups = cmap_groups(original)?;
    if glyph_for(&groups, 'm' as u32).is_none() {
        let glyph = glyph_for(&groups, ' ' as u32).unwrap_or(1);
        let at = groups.partition_point(|group| group.0 < 'm' as u32);
        groups.insert(at, ('m' as u32, 'm' as u32, glyph));
    }
    let mut out = Vec::with_capacity(28 + 12 * groups.len());
    out.extend_from_slice(&[0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12]);
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(16 + 12 * groups.len() as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (start, end, glyph) in groups {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&glyph.to_be_bytes());
    }
    Some(out)
}

/// A name table naming the face `FAMILY` with PostScript name `POSTSCRIPT`.
fn patched_name() -> Vec<u8> {
    let names: [(u16, &str); 4] = [(1, FAMILY), (2, "Regular"), (4, FAMILY), (6, POSTSCRIPT)];
    let strings: Vec<Vec<u8>> = names.iter().map(|(_, text)| text.encode_utf16().flat_map(u16::to_be_bytes).collect()).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(names.len() as u16).to_be_bytes());
    out.extend_from_slice(&(6 + 12 * names.len() as u16).to_be_bytes());
    let mut offset = 0u16;
    for ((id, _), string) in names.iter().zip(&strings) {
        for value in [3u16, 1, 0x409, *id, string.len() as u16, offset] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        offset += string.len() as u16;
    }
    for string in strings {
        out.extend_from_slice(&string);
    }
    out
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// A new table directory plus the patched cmap and name, padded to a whole
/// number of pages. The original font follows it byte for byte, so every
/// other table keeps its data and moves by the prefix length.
fn prefix(font: &[u8], page: usize) -> Option<Vec<u8>> {
    let all = tables(font)?;
    let cmap = patched_cmap(table(font, &all, b"cmap")?)?;
    let name = patched_name();
    let mut entries: Vec<(Table, Option<&[u8]>)> = all.iter().filter(|table| &table.tag != b"cmap" && &table.tag != b"name").map(|table| (*table, None)).collect();
    entries.push((Table { tag: *b"cmap", checksum: checksum(&cmap), offset: 0, length: cmap.len() as u32 }, Some(&cmap)));
    entries.push((Table { tag: *b"name", checksum: checksum(&name), offset: 0, length: name.len() as u32 }, Some(&name)));
    entries.sort_by_key(|(table, _)| table.tag);

    let count = entries.len();
    let directory = 12 + 16 * count;
    let body = directory.next_multiple_of(4) + cmap.len().next_multiple_of(4) + name.len();
    let size = body.next_multiple_of(page);

    let mut out = vec![0u8; size];
    let power = 1usize << (usize::BITS - 1 - count.leading_zeros());
    out[0..4].copy_from_slice(&u32_at(font, 0)?.to_be_bytes());
    out[4..6].copy_from_slice(&(count as u16).to_be_bytes());
    out[6..8].copy_from_slice(&((power * 16) as u16).to_be_bytes());
    out[8..10].copy_from_slice(&(power.trailing_zeros() as u16).to_be_bytes());
    out[10..12].copy_from_slice(&((count * 16 - power * 16) as u16).to_be_bytes());
    let mut next = directory.next_multiple_of(4);
    for (index, (mut table, data)) in entries.into_iter().enumerate() {
        match data {
            Some(data) => {
                table.offset = next as u32;
                out[next..next + data.len()].copy_from_slice(data);
                next = (next + data.len()).next_multiple_of(4);
            }
            None => table.offset = u32::try_from(size + table.offset as usize).ok()?,
        }
        let at = 12 + 16 * index;
        out[at..at + 4].copy_from_slice(&table.tag);
        out[at + 4..at + 8].copy_from_slice(&table.checksum.to_be_bytes());
        out[at + 8..at + 12].copy_from_slice(&table.offset.to_be_bytes());
        out[at + 12..at + 16].copy_from_slice(&table.length.to_be_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    /// A two-table font: a format 12 cmap mapping U+1F600 and space, and a glyf stand-in.
    fn font() -> Vec<u8> {
        let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0];
        cmap.extend_from_slice(&(16u32 + 24).to_be_bytes());
        cmap.extend_from_slice(&0u32.to_be_bytes());
        cmap.extend_from_slice(&2u32.to_be_bytes());
        for (start, end, glyph) in [(0x20u32, 0x20u32, 3u32), (0x1F600, 0x1F601, 7)] {
            cmap.extend_from_slice(&start.to_be_bytes());
            cmap.extend_from_slice(&end.to_be_bytes());
            cmap.extend_from_slice(&glyph.to_be_bytes());
        }
        let glyf = b"GLYPHDATA!!!".to_vec();
        let mut font = vec![0, 1, 0, 0, 0, 2, 0, 32, 0, 1, 0, 0];
        let cmap_at = 12 + 32;
        let glyf_at = cmap_at + cmap.len();
        for (tag, at, len) in [(b"cmap", cmap_at, cmap.len()), (b"glyf", glyf_at, glyf.len())] {
            font.extend_from_slice(tag);
            font.extend_from_slice(&0u32.to_be_bytes());
            font.extend_from_slice(&(at as u32).to_be_bytes());
            font.extend_from_slice(&(len as u32).to_be_bytes());
        }
        font.extend_from_slice(&cmap);
        font.extend_from_slice(&glyf);
        font
    }

    #[test]
    fn the_prefix_maps_m_renames_the_face_and_points_at_the_original_tables() {
        let original = font();
        let head = prefix(&original, 4096).unwrap();
        assert_eq!(head.len() % 4096, 0);
        let mut combined = head.clone();
        combined.extend_from_slice(&original);

        let all = tables(&combined).unwrap();
        assert_eq!(all.iter().map(|table| table.tag).collect::<Vec<_>>(), vec![*b"cmap", *b"glyf", *b"name"]);
        assert_eq!(table(&combined, &all, b"glyf").unwrap(), b"GLYPHDATA!!!");

        let groups = cmap_groups(table(&combined, &all, b"cmap").unwrap()).unwrap();
        assert_eq!(glyph_for(&groups, 'm' as u32), Some(3));
        assert_eq!(glyph_for(&groups, 0x1F601), Some(8));

        let name = table(&combined, &all, b"name").unwrap();
        let postscript: Vec<u8> = POSTSCRIPT.encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert!(name.windows(postscript.len()).any(|window| window == postscript.as_slice()));
    }

    #[test]
    fn emoji_clusters_are_found_and_neighbours_merge() {
        let text = "hi \u{2764}\u{FE0F}\u{1F44D} ok \u{1F1EA}\u{1F1F8}";
        let ranges = emoji_ranges(text);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].clone()], "\u{2764}\u{FE0F}\u{1F44D}");
        assert_eq!(&text[ranges[1].clone()], "\u{1F1EA}\u{1F1F8}");
        assert!(emoji_ranges("# 1 plain").is_empty());
    }

    /// Patches a real system font and checks every table still reads back
    /// the same bytes and every mapping survives.
    #[test]
    fn a_real_font_survives_the_patch() {
        let candidates = ["/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Supplemental/Courier New.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/run/current-system/sw/share/X11/fonts/DejaVuSans.ttf"];
        let Some(original) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else { return };
        let head = prefix(&original, 4096).unwrap();
        let mut combined = head.clone();
        combined.extend_from_slice(&original);
        let (before, after) = (tables(&original).unwrap(), tables(&combined).unwrap());
        for table_before in before.iter().filter(|table| &table.tag != b"cmap" && &table.tag != b"name") {
            assert_eq!(table(&original, &before, &table_before.tag), table(&combined, &after, &table_before.tag));
        }
        let old = cmap_groups(table(&original, &before, b"cmap").unwrap()).unwrap();
        let new = cmap_groups(table(&combined, &after, b"cmap").unwrap()).unwrap();
        for char in ['A', 'z', '0', '\u{e9}', '\u{20ac}'] {
            assert_eq!(glyph_for(&old, char as u32), glyph_for(&new, char as u32));
        }
        assert!(glyph_for(&new, 'm' as u32).is_some());
    }

    #[test]
    fn a_format_4_cmap_is_read_into_groups() {
        // One segment 0x41..=0x43 with delta 1 (glyphs 0x42..=0x44), then the 0xFFFF terminator.
        let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 1, 0, 0, 0, 12, 0, 4, 0, 32, 0, 0, 0, 4, 0, 4, 0, 1, 0, 0];
        for value in [0x43u16, 0xFFFF, 0, 0x41, 0xFFFF, 1, 1, 0, 0] {
            cmap.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(cmap_groups(&cmap).unwrap(), vec![(0x41, 0x43, 0x42)]);
    }
}
