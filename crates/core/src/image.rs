//! Port of packages/core/src/image.ts: sizes from file headers (EXIF
//! orientation included) and the square thumbnails Linux needs because GPUI
//! does not clip an `object-fit: cover` image there.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

/// JPEG EXIF orientation 1 to 8, or None.
pub fn exif_orientation(bytes: &[u8]) -> Option<u8> {
    let _ = bytes;
    unimplemented!()
}

/// PNG, JPEG (turned upright for orientation 5 to 8), GIF, WebP, BMP or an SVG's declared size.
pub fn image_size_from_bytes(bytes: &[u8]) -> Option<ImageSize> {
    let _ = bytes;
    unimplemented!()
}

pub fn is_heif(bytes: &[u8]) -> bool {
    let _ = bytes;
    unimplemented!()
}

/// HEIF still to PNG with ffmpeg, merging the sticker's second stream as alpha, then a plain decode. False without ffmpeg.
pub async fn heif_to_png(source: &Path, target: &Path) -> bool {
    let _ = (source, target);
    unimplemented!()
}

/// Reads at most the first 256 KiB. Accepts a path or a `data:` URL.
pub async fn image_size(source: &str) -> Option<ImageSize> {
    let _ = source;
    unimplemented!()
}

/// Scales down to fit, never up, never cropping.
pub fn fit_inside(size: ImageSize, max_width: u32, max_height: u32) -> ImageSize {
    let _ = (size, max_width, max_height);
    unimplemented!()
}

/// Centre square of a JPEG or PNG, box-filtered to at most `side` px, as PNG bytes. Default side 256.
pub fn square_thumbnail(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    let _ = (bytes, side);
    unimplemented!()
}
