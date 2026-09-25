//! Port of packages/core/src/image.ts: sizes from file headers (EXIF
//! orientation included) and the square thumbnails Linux needs because GPUI
//! does not clip an `object-fit: cover` image there.

use std::path::Path;
use std::process::Stdio;
use std::sync::LazyLock;

use base64::Engine as _;
use image::{ImageEncoder, RgbaImage};
use regex::Regex;
use tokio::io::AsyncReadExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

fn u16be(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from(*bytes.get(offset)?) << 8 | u32::from(*bytes.get(offset + 1)?))
}

fn u16le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from(*bytes.get(offset)?) | u32::from(*bytes.get(offset + 1)?) << 8)
}

fn u32be(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn u32le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn has(bytes: &[u8], offset: usize, text: &[u8]) -> bool {
    bytes.get(offset..offset + text.len()) == Some(text)
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0xff && bytes[1] == 0xd8
}

struct Segment {
    marker: u8,
    offset: usize,
    length: usize,
}

/// Every marker segment up to and including the start of scan.
fn jpeg_segments(bytes: &[u8]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut offset = 2;
    while offset + 9 < bytes.len() {
        if bytes[offset] != 0xff {
            offset += 1;
            continue;
        }
        let marker = bytes[offset + 1];
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            offset += 2;
            continue;
        }
        let length = u16be(bytes, offset + 2).unwrap_or(0) as usize;
        out.push(Segment { marker, offset, length });
        if marker == 0xda {
            break;
        }
        offset += 2 + length;
    }
    out
}

/// The Orientation tag (0x0112) of an APP1 segment that carries Exif, or None: XMP travels in APP1 too.
fn orientation_from_app1(bytes: &[u8], offset: usize, length: usize) -> Option<u8> {
    let start = offset + 4;
    let end = bytes.len().min(offset + 2 + length);
    if start + 14 > end || !has(bytes, start, b"Exif") {
        return None;
    }
    let tiff = start + 6;
    let little = if has(bytes, tiff, b"II") {
        true
    } else if has(bytes, tiff, b"MM") {
        false
    } else {
        return None;
    };
    let u16 = |at: usize| if little { u16le(bytes, at) } else { u16be(bytes, at) };
    let u32 = |at: usize| if little { u32le(bytes, at) } else { u32be(bytes, at) };
    let ifd = tiff + u32(tiff + 4)? as usize;
    if ifd + 2 > end {
        return None;
    }
    let count = u16(ifd)? as usize;
    for index in 0..count {
        let entry = ifd + 2 + index * 12;
        if entry + 12 > end {
            return None;
        }
        if u16(entry)? != 0x0112 {
            continue;
        }
        let value = u16(entry + 8)?;
        return (1..=8).contains(&value).then_some(value as u8);
    }
    None
}

/// JPEG EXIF orientation 1 to 8, or None. 5 to 8 mean the pixels are stored a
/// quarter turn from how the photo is meant to be seen; the renderer turns
/// them upright, so their box has to turn with them.
pub fn exif_orientation(bytes: &[u8]) -> Option<u8> {
    if !is_jpeg(bytes) {
        return None;
    }
    jpeg_segments(bytes)
        .into_iter()
        .filter(|segment| segment.marker == 0xe1)
        .find_map(|segment| orientation_from_app1(bytes, segment.offset, segment.length))
}

/// JS `parseFloat`: the longest leading decimal number, so "600px" is 600.
fn parse_float_prefix(text: &str) -> Option<f64> {
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?").unwrap());
    NUMBER.find(text.trim_start()).and_then(|found| found.as_str().parse().ok())
}

fn js_round(value: f64) -> u32 {
    (value + 0.5).floor() as u32
}

fn svg_size(source: &str) -> Option<ImageSize> {
    static OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<svg[^>]*>").unwrap());
    static WIDTH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bwidth\s*=\s*["']([^"']+)["']"#).unwrap());
    static HEIGHT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bheight\s*=\s*["']([^"']+)["']"#).unwrap());
    static VIEW_BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bviewBox\s*=\s*["']([^"']+)["']"#).unwrap());
    static SEPARATOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s,]+").unwrap());

    let open = OPEN.find(source)?.as_str();
    let attr = |pattern: &Regex| {
        let value = parse_float_prefix(pattern.captures(open)?.get(1)?.as_str())?;
        (value.is_finite() && value > 0.0).then_some(value)
    };
    if let (Some(width), Some(height)) = (attr(&WIDTH), attr(&HEIGHT)) {
        return Some(ImageSize { width: js_round(width), height: js_round(height) });
    }
    let view_box = VIEW_BOX.captures(open)?.get(1)?.as_str().trim();
    let numbers: Vec<f64> = SEPARATOR.split(view_box).map(|part| part.parse().unwrap_or(f64::NAN)).collect();
    if numbers.len() == 4 && numbers[2] > 0.0 && numbers[3] > 0.0 {
        return Some(ImageSize { width: js_round(numbers[2]), height: js_round(numbers[3]) });
    }
    None
}

/// PNG, JPEG (turned upright for orientation 5 to 8), GIF, WebP, BMP or an SVG's declared size.
pub fn image_size_from_bytes(bytes: &[u8]) -> Option<ImageSize> {
    if bytes.len() >= 24 && bytes[0] == 0x89 && has(bytes, 1, b"PNG") {
        return Some(ImageSize { width: u32be(bytes, 16)?, height: u32be(bytes, 20)? });
    }
    if bytes.len() >= 10 && has(bytes, 0, b"GIF8") {
        return Some(ImageSize { width: u16le(bytes, 6)?, height: u16le(bytes, 8)? });
    }
    if is_jpeg(bytes) {
        let mut orientation = 1;
        for Segment { marker, offset, length } in jpeg_segments(bytes) {
            if marker == 0xe1 {
                orientation = orientation_from_app1(bytes, offset, length).unwrap_or(orientation);
            }
            let is_sof = (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc;
            if !is_sof {
                continue;
            }
            let (height, width) = (u16be(bytes, offset + 5)?, u16be(bytes, offset + 7)?);
            return Some(if orientation >= 5 { ImageSize { width: height, height: width } } else { ImageSize { width, height } });
        }
        return None;
    }
    if bytes.len() >= 30 && has(bytes, 0, b"RIFF") && has(bytes, 8, b"WEBP") {
        if has(bytes, 12, b"VP8 ") {
            return Some(ImageSize { width: u16le(bytes, 26)? & 0x3fff, height: u16le(bytes, 28)? & 0x3fff });
        }
        if has(bytes, 12, b"VP8L") {
            let b = u32le(bytes, 21)?;
            return Some(ImageSize { width: (b & 0x3fff) + 1, height: ((b >> 14) & 0x3fff) + 1 });
        }
        if has(bytes, 12, b"VP8X") {
            let u24 = |at: usize| u32::from(bytes[at]) | u32::from(bytes[at + 1]) << 8 | u32::from(bytes[at + 2]) << 16;
            return Some(ImageSize { width: u24(24) + 1, height: u24(27) + 1 });
        }
        return None;
    }
    if bytes.len() >= 26 && has(bytes, 0, b"BM") {
        return Some(ImageSize { width: u32le(bytes, 18)?, height: (u32le(bytes, 22)? as i32).unsigned_abs() });
    }
    let head = &bytes[..bytes.len().min(512)];
    if head.windows(4).any(|window| window == b"<svg") {
        return svg_size(&String::from_utf8_lossy(bytes));
    }
    None
}

const HEIF_BRANDS: [&[u8; 4]; 10] = [b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1"];

/// An ISO base media file whose major brand is one of the HEIF family.
pub fn is_heif(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && has(bytes, 4, b"ftyp") && HEIF_BRANDS.iter().any(|brand| has(bytes, 8, *brand))
}

/// HEIF still to PNG with ffmpeg, merging the sticker's second stream as alpha, then a plain decode. False without ffmpeg.
/// `target` needs a `.png` extension: ffmpeg picks the muxer from it.
pub async fn heif_to_png(source: &Path, target: &Path) -> bool {
    let attempts: [&[&str]; 2] = [&["-filter_complex", "[0:v:0][0:v:1]alphamerge"], &[]];
    for filter in attempts {
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(source)
            .args(filter)
            .args(["-frames:v", "1"])
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status()
            .await;
        match status {
            Err(_) => return false,
            Ok(status) if status.success() && tokio::fs::metadata(target).await.is_ok() => return true,
            Ok(_) => {}
        }
    }
    false
}

/// Reads at most the first 256 KiB. Accepts a path or a `data:` URL.
pub async fn image_size(source: &str) -> Option<ImageSize> {
    if let Some(rest) = source.strip_prefix("data:") {
        let (meta, payload) = rest.split_once(',')?;
        let bytes = if meta.ends_with(";base64") {
            base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()?
        } else {
            percent_encoding::percent_decode_str(payload).collect()
        };
        return image_size_from_bytes(&bytes);
    }
    let file = tokio::fs::File::open(source).await.ok()?;
    let mut head = Vec::with_capacity(64 * 1024);
    file.take(256 * 1024).read_to_end(&mut head).await.ok()?;
    image_size_from_bytes(&head)
}

/// Scales down to fit, never up, never cropping.
pub fn fit_inside(size: ImageSize, max_width: u32, max_height: u32) -> ImageSize {
    let scale = (f64::from(max_width) / f64::from(size.width)).min(f64::from(max_height) / f64::from(size.height)).min(1.0);
    ImageSize {
        width: js_round(f64::from(size.width) * scale).max(1),
        height: js_round(f64::from(size.height) * scale).max(1),
    }
}

/// The centre square of `image`, box-filtered down to `side` pixels a side, never scaled up.
pub fn square_crop(image: &RgbaImage, side: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    let source = width.min(height) as u64;
    let ox = (width as u64 - source) / 2;
    let oy = (height as u64 - source) / 2;
    let target = (side as u64).min(source);
    let data = image.as_raw();
    let mut out = RgbaImage::new(target as u32, target as u32);
    for ty in 0..target {
        let y0 = oy + ty * source / target;
        let y1 = (y0 + 1).max(oy + (ty + 1) * source / target);
        for tx in 0..target {
            let x0 = ox + tx * source / target;
            let x1 = (x0 + 1).max(ox + (tx + 1) * source / target);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let index = ((y * width as u64 + x) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(data[index + channel]);
                    }
                    count += 1;
                }
            }
            let pixel = sum.map(|total| ((total as f64 / count as f64) + 0.5).floor() as u8);
            out.put_pixel(tx as u32, ty as u32, image::Rgba(pixel));
        }
    }
    out
}

/// Centre square of a JPEG or PNG, box-filtered to at most `side` px, as PNG bytes. Default side 256.
/// Decodes the whole bitmap, so callers on the runtime run it under `spawn_blocking`.
pub fn square_thumbnail(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    let format = match bytes {
        [0xff, 0xd8, ..] => image::ImageFormat::Jpeg,
        [0x89, 0x50, ..] => image::ImageFormat::Png,
        _ => return None,
    };
    let decoded = image::load_from_memory_with_format(bytes, format).ok()?.to_rgba8();
    let cropped = square_crop(&decoded, side);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(cropped.as_raw(), cropped.width(), cropped.height(), image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52];
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.resize(33, 0);
        bytes
    }

    fn sof(width: u32, height: u32) -> Vec<u8> {
        vec![0xff, 0xc0, 0, 11, 8, (height >> 8) as u8, height as u8, (width >> 8) as u8, width as u8, 1, 1, 0x11, 0]
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let app0 = [0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0];
        [&[0xff, 0xd8][..], &app0, &sof(width, height), &[0xff, 0xda]].concat()
    }

    fn jpeg_with_orientation(width: u32, height: u32, orientation: u8) -> Vec<u8> {
        let exif = [
            0x45, 0x78, 0x69, 0x66, 0, 0, 0x4d, 0x4d, 0, 0x2a, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0, 0, 0, 0, 0,
        ];
        let app1 = [&[0xff, 0xe1, 0, (exif.len() + 2) as u8][..], &exif].concat();
        [&[0xff, 0xd8][..], &app1, &sof(width, height), &[0xff, 0xda]].concat()
    }

    fn size(width: u32, height: u32) -> Option<ImageSize> {
        Some(ImageSize { width, height })
    }

    #[test]
    fn reads_png() {
        assert_eq!(image_size_from_bytes(&png(600, 1300)), size(600, 1300));
    }

    #[test]
    fn reads_jpeg_after_an_app0_segment() {
        assert_eq!(image_size_from_bytes(&jpeg(1080, 1920)), size(1080, 1920));
    }

    #[test]
    fn turns_a_jpeg_stored_a_quarter_turn_from_upright() {
        assert_eq!(image_size_from_bytes(&jpeg_with_orientation(4032, 3024, 6)), size(3024, 4032));
    }

    #[test]
    fn keeps_an_upright_jpeg_as_stored() {
        assert_eq!(image_size_from_bytes(&jpeg_with_orientation(4032, 3024, 1)), size(4032, 3024));
    }

    #[test]
    fn reads_gif() {
        let bytes = [b"GIF89a".as_slice(), &[0xf4, 0x01, 0x2c, 0x01, 0, 0, 0]].concat();
        assert_eq!(image_size_from_bytes(&bytes), size(500, 300));
    }

    #[test]
    fn reads_svg_width_and_height() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="1300" viewBox="0 0 600 1300"></svg>"#;
        assert_eq!(image_size_from_bytes(svg), size(600, 1300));
    }

    #[test]
    fn returns_none_for_unknown_bytes() {
        assert_eq!(image_size_from_bytes(&[1, 2, 3, 4]), None);
    }

    #[test]
    fn reads_the_orientation_tag() {
        assert_eq!(exif_orientation(&jpeg_with_orientation(100, 50, 8)), Some(8));
    }

    #[test]
    fn orientation_is_none_without_exif() {
        assert_eq!(exif_orientation(&jpeg(100, 50)), None);
    }

    #[tokio::test]
    async fn image_size_handles_data_urls() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 200"></svg>"#;
        let url = format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg));
        assert_eq!(image_size(&url).await, size(400, 200));
    }

    #[test]
    fn fit_inside_scales_tall_images_down_to_the_height_cap() {
        assert_eq!(fit_inside(ImageSize { width: 600, height: 1300 }, 320, 420), ImageSize { width: 194, height: 420 });
    }

    #[test]
    fn fit_inside_never_scales_up() {
        assert_eq!(fit_inside(ImageSize { width: 100, height: 50 }, 320, 420), ImageSize { width: 100, height: 50 });
    }

    /// width x height RGBA, the left half red and the right half blue.
    fn pixels(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, _| {
            if x < width / 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) }
        })
    }

    #[test]
    fn square_crop_keeps_the_centre_of_a_wide_image_and_shrinks_it() {
        let out = square_crop(&pixels(400, 100), 50);
        assert_eq!(out.dimensions(), (50, 50));
        assert_eq!(out.as_raw()[0], 255);
        assert_eq!(out.as_raw()[(50 - 1) * 4 + 2], 255);
    }

    #[test]
    fn square_crop_never_scales_up() {
        assert_eq!(square_crop(&pixels(30, 40), 256).width(), 30);
    }

    fn encode(image: &RgbaImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        match format {
            image::ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image.clone()).to_rgb8().write_to(&mut out, format).unwrap(),
            _ => image.write_to(&mut out, format).unwrap(),
        }
        out.into_inner()
    }

    #[test]
    fn square_thumbnail_turns_a_portrait_jpeg_into_a_square_png() {
        let out = square_thumbnail(&encode(&pixels(60, 120), image::ImageFormat::Jpeg), 32).unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (32, 32));
    }

    #[test]
    fn square_thumbnail_reads_png_too() {
        let out = square_thumbnail(&encode(&pixels(20, 10), image::ImageFormat::Png), 256).unwrap();
        assert_eq!(image::load_from_memory(&out).unwrap().width(), 10);
    }

    #[test]
    fn square_thumbnail_leaves_anything_else_alone() {
        assert_eq!(square_thumbnail(&[1, 2, 3, 4], 256), None);
    }

    #[test]
    fn detects_heif_brands() {
        let mut bytes = vec![0, 0, 0, 24];
        bytes.extend(b"ftypheic");
        assert!(is_heif(&bytes));
        assert!(!is_heif(&jpeg(1, 1)));
    }
}
