//! Derived media the thread paints, ported from the ffmpeg helpers in
//! apps/desktop/src/ui/{bubble,gif,ffmpeg}.ts. Lives in core so it runs on the
//! runtime, off the GPUI thread. Stills are done in process with the `image`
//! crate; ffmpeg is only needed for video posters and HEIF. Every output is
//! written beside its source in the attachment cache and named from the shared
//! (SHA-1) file name, so identical attachments share one derived file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use image::AnimationDecoder;
use tokio::sync::{OnceCell, Semaphore};

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

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dir.join(format!("{name}{suffix}"))
}

/// ffmpeg reads a still the way it is stored, so the EXIF turn goes in by hand ahead of the cut.
fn upright_filter(orientation: Option<u8>) -> &'static str {
    match orientation {
        Some(2) => "hflip,",
        Some(3) => "hflip,vflip,",
        Some(4) => "vflip,",
        Some(5) => "transpose=0,",
        Some(6) => "transpose=1,",
        Some(7) => "transpose=3,",
        Some(8) => "transpose=2,",
        _ => "",
    }
}

async fn read_head(path: &Path, max: usize) -> Option<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let mut buf = vec![0u8; max];
    let n = file.read(&mut buf).await.ok()?;
    buf.truncate(n);
    Some(buf)
}

async fn read_exif_orientation(path: &Path) -> Option<u8> {
    let bytes = read_head(path, 256 * 1024).await?;
    crate::image::exif_orientation(&bytes)
}

/// Serialises ffmpeg (two at a time app-wide) and runs each output file's job once however many callers ask.
pub struct MediaWorker {
    ffmpeg_available: bool,
    slot: Semaphore,
    inflight: parking_lot::Mutex<HashMap<PathBuf, Arc<OnceCell<Option<PathBuf>>>>>,
}

impl MediaWorker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { ffmpeg_available: which::which("ffmpeg").is_ok(), slot: Semaphore::new(2), inflight: parking_lot::Mutex::new(HashMap::new()) })
    }

    /// Runs `job` at most once per `target`, however many callers ask concurrently for the same output file.
    async fn once<Fut>(&self, target: &Path, job: Fut) -> Option<PathBuf>
    where
        Fut: std::future::Future<Output = Option<PathBuf>>,
    {
        let cell = { self.inflight.lock().entry(target.to_path_buf()).or_insert_with(|| Arc::new(OnceCell::new())).clone() };
        let result = cell.get_or_init(move || job).await.clone();
        self.inflight.lock().remove(target);
        result
    }

    async fn run_ffmpeg_still(&self, source: &Path, filter: &str, quality: u8, target: &Path) -> Option<PathBuf> {
        let _permit = self.slot.acquire().await.ok()?;
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-y", "-noautorotate", "-i"])
            .arg(source)
            .args(["-vf", filter, "-frames:v", "1", "-q:v", &quality.to_string()])
            .arg(target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .ok()?;
        if status.success() && tokio::fs::try_exists(target).await.unwrap_or(false) {
            Some(target.to_path_buf())
        } else {
            None
        }
    }

    /// A still whose long edge exceeds PREVIEW_MAX_EDGE, EXIF-uprighted and scaled down; None when it needs none.
    pub async fn preview(&self, image: &Path) -> Option<PathBuf> {
        let target = sibling(image, ".preview.jpg");
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Some(target);
        }
        if !self.ffmpeg_available {
            return None;
        }
        let image = image.to_path_buf();
        let target_job = target.clone();
        self.once(&target, async move {
            let orientation = read_exif_orientation(&image).await;
            let fit = format!("scale=w='min(iw,{PREVIEW_MAX_EDGE})':h='min(ih,{PREVIEW_MAX_EDGE})':force_original_aspect_ratio=decrease:flags=area");
            let filter = format!("{}{fit}", upright_filter(orientation));
            self.run_ffmpeg_still(&image, &filter, 4, &target_job).await
        })
        .await
    }

    /// Centre crop to `aspect` (width / height) scaled to `side`, cached per aspect.
    pub async fn tile(&self, image: &Path, aspect: f32, side: u32) -> Option<PathBuf> {
        let target = sibling(image, &format!(".tile-{aspect}x1-{side}.jpg"));
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Some(target);
        }
        if !self.ffmpeg_available {
            return None;
        }
        let image = image.to_path_buf();
        let target_job = target.clone();
        self.once(&target, async move {
            let orientation = read_exif_orientation(&image).await;
            let width = (side as f32 * aspect).round() as u32;
            let cut = format!("crop=w='min(iw,ih*{aspect})':h='min(ih,iw/{aspect})',scale={width}:{side}");
            let filter = format!("{}{cut}", upright_filter(orientation));
            self.run_ffmpeg_still(&image, &filter, 3, &target_job).await
        })
        .await
    }

    /// The photo's bottom outer corner (12% by 14%, right when `from_me`) at 2x the tail size, uprighted.
    pub async fn tail_cut(&self, image: &Path, from_me: bool) -> Option<PathBuf> {
        if !self.ffmpeg_available {
            return None;
        }
        let side = if from_me { "right" } else { "left" };
        let target = sibling(image, &format!(".tail-{side}.jpg"));
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Some(target);
        }
        let image = image.to_path_buf();
        let target_job = target.clone();
        self.once(&target, async move {
            let orientation = read_exif_orientation(&image).await;
            let x = if from_me { "iw-out_w" } else { "0" };
            let cut = format!("crop=w='iw*0.12':h='ih*0.14':x='{x}':y='ih-out_h',scale={}:{}", 2 * TAIL_WIDTH, 2 * TAIL_HEIGHT);
            let filter = format!("{}{cut}", upright_filter(orientation));
            self.run_ffmpeg_still(&image, &filter, 3, &target_job).await
        })
        .await
    }

    /// Frame at 0.5 s, 640 px wide, `<attachments>/<guid>.poster.jpg`. Needs ffmpeg.
    pub async fn video_poster(&self, video: &Path, guid: &str) -> Option<PathBuf> {
        let target = video.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new(".")).join(format!("{guid}.poster.jpg"));
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Some(target);
        }
        if !self.ffmpeg_available {
            return None;
        }
        let video = video.to_path_buf();
        let target_job = target.clone();
        self.once(&target, async move {
            let _permit = self.slot.acquire().await.ok()?;
            let status = tokio::process::Command::new("ffmpeg")
                .args(["-y", "-ss", "0.5", "-i"])
                .arg(&video)
                .args(["-frames:v", "1", "-vf", "scale=640:-1"])
                .arg(&target_job)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .ok()?;
            if status.success() && tokio::fs::try_exists(&target_job).await.unwrap_or(false) {
                Some(target_job.clone())
            } else {
                None
            }
        })
        .await
    }

    /// Every frame with its delay. None for a GIF with fewer than two frames, which is painted as a still.
    pub async fn decode_gif(&self, gif: &Path) -> Option<Arc<GifAnimation>> {
        let path = gif.to_path_buf();
        let animation = tokio::task::spawn_blocking(move || decode_gif_sync(&path)).await.ok()??;
        Some(Arc::new(animation))
    }
}

fn decode_gif_sync(path: &Path) -> Option<GifAnimation> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)).ok()?;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut frames = Vec::new();
    for frame in decoder.into_frames() {
        let frame = frame.ok()?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay_ms = if denom == 0 { GIF_SLOW_FRAME.as_millis() as u32 } else { (numer / denom).max(1) };
        let delay = if delay_ms < GIF_MIN_DELAY.as_millis() as u32 { GIF_SLOW_FRAME } else { Duration::from_millis(delay_ms as u64) };
        let buffer = frame.into_buffer();
        width = buffer.width();
        height = buffer.height();
        let mut bgra = buffer.into_raw();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        frames.push(GifFrame { bgra, delay });
    }
    if frames.len() < 2 {
        return None;
    }
    Some(GifAnimation { width, height, frames })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::GifEncoder;
    use image::{Frame, Rgba, RgbaImage};

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("messages-media-test-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_gif(path: &Path, colors: &[[u8; 4]]) {
        let mut file = std::fs::File::create(path).unwrap();
        let mut encoder = GifEncoder::new(&mut file);
        for color in colors {
            let image = RgbaImage::from_pixel(2, 2, Rgba(*color));
            encoder.encode_frame(Frame::new(image)).unwrap();
        }
    }

    #[test]
    fn sibling_names_the_derived_file_after_the_source() {
        let path = sibling(Path::new("/cache/attachments/abc123.jpg"), ".preview.jpg");
        assert_eq!(path, PathBuf::from("/cache/attachments/abc123.jpg.preview.jpg"));
    }

    #[test]
    fn upright_filter_maps_every_exif_orientation() {
        assert_eq!(upright_filter(None), "");
        assert_eq!(upright_filter(Some(1)), "");
        assert_eq!(upright_filter(Some(6)), "transpose=1,");
        assert_eq!(upright_filter(Some(8)), "transpose=2,");
    }

    #[tokio::test]
    async fn decodes_multiple_frames_and_swaps_to_bgra() {
        let dir = tempdir();
        let path = dir.join("anim.gif");
        write_gif(&path, &[[255, 0, 0, 255], [0, 0, 255, 255]]);
        let worker = MediaWorker::new();
        let anim = worker.decode_gif(&path).await.unwrap();
        assert_eq!(anim.frames.len(), 2);
        assert_eq!(anim.width, 2);
        assert_eq!(anim.height, 2);
        // Frame 0 was solid red (255,0,0,255) RGBA; BGRA byte order reads blue,green,red,alpha.
        assert_eq!(&anim.frames[0].bgra[0..4], &[0, 0, 255, 255]);
    }

    #[tokio::test]
    async fn a_single_frame_gif_decodes_to_none_so_it_paints_as_a_still() {
        let dir = tempdir();
        let path = dir.join("still.gif");
        write_gif(&path, &[[10, 20, 30, 255]]);
        let worker = MediaWorker::new();
        assert!(worker.decode_gif(&path).await.is_none());
    }
}
