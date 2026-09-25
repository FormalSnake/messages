//! Derived media the thread paints, ported from the ffmpeg helpers in
//! apps/desktop/src/ui/{bubble,gif,ffmpeg}.ts. Lives in core so it runs on the
//! runtime, off the GPUI thread. Stills are done in process with the `image`
//! crate; ffmpeg is only needed for video posters and HEIF. Every output is
//! written beside its source in the attachment cache and named from the shared
//! (SHA-1) file name, so identical attachments share one derived file.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Long edge above which a still gets a downscaled preview before it is painted.
pub const PREVIEW_MAX_EDGE: u32 = 840;
/// Side of a photo grid tile.
pub const TILE_SIDE: u32 = 512;
/// The bubble tail lobe, in logical px. Cuts are rendered at 2x.
pub const TAIL_WIDTH: u32 = 14;
pub const TAIL_HEIGHT: u32 = 16;
/// Frame delays under this are shown at SLOW_FRAME, as browsers do.
pub const GIF_MIN_DELAY: Duration = Duration::from_millis(20);
pub const GIF_SLOW_FRAME: Duration = Duration::from_millis(100);

/// One animated GIF, decoded once per shared path. Frames are BGRA as GPUI's
/// `RenderImage` wants them. The desktop keeps these in a bounded LRU.
pub struct GifAnimation {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<GifFrame>,
}

pub struct GifFrame {
    pub bgra: Vec<u8>,
    pub delay: Duration,
}

impl GifAnimation {
    pub fn byte_size(&self) -> usize {
        self.frames.iter().map(|frame| frame.bgra.len()).sum()
    }
}

/// Serialises ffmpeg (two at a time app-wide) and runs each output file's job once however many callers ask.
pub struct MediaWorker {
    _private: (),
}

impl MediaWorker {
    pub fn new() -> Arc<Self> {
        unimplemented!()
    }

    /// A still whose long edge exceeds PREVIEW_MAX_EDGE, EXIF-uprighted and scaled down; None when it needs none.
    pub async fn preview(&self, image: &Path) -> Option<PathBuf> {
        let _ = image;
        unimplemented!()
    }

    /// Centre crop to `aspect` (width / height) scaled to `side`, cached per aspect.
    pub async fn tile(&self, image: &Path, aspect: f32, side: u32) -> Option<PathBuf> {
        let _ = (image, aspect, side);
        unimplemented!()
    }

    /// The photo's bottom outer corner (12% by 14%, right when `from_me`) at 2x the tail size, uprighted.
    pub async fn tail_cut(&self, image: &Path, from_me: bool) -> Option<PathBuf> {
        let _ = (image, from_me);
        unimplemented!()
    }

    /// Frame at 0.5 s, 640 px wide, `<attachments>/<guid>.poster.jpg`. Needs ffmpeg.
    pub async fn video_poster(&self, video: &Path, guid: &str) -> Option<PathBuf> {
        let _ = (video, guid);
        unimplemented!()
    }

    /// Every frame with its delay. None for a GIF with fewer than two frames, which is painted as a still.
    pub async fn decode_gif(&self, gif: &Path) -> Option<Arc<GifAnimation>> {
        let _ = gif;
        unimplemented!()
    }
}
