//! In-app video playback on the ffmpeg binary the poster and HEIF paths
//! already need. One ffmpeg decodes the picture to raw BGRA frames at the
//! size they will be shown, piped through stdout; a second decodes the sound
//! to f32 at the device's rate for `audio::AudioSink`. The frame task paces
//! itself against the sound the device has played (or wall time when there is
//! none), drops frames it is late for, and hands the rest to the UI through a
//! bounded channel. Pausing stops the clock and stops reading the pipes, so
//! ffmpeg blocks on a full pipe and burns nothing. A seek is a fresh
//! `Playback` started at the new position.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::audio::AudioSink;

/// The picture the frame task waits for the clock to reach before it lets a
/// frame through. Under this the frame is on time.
const SLACK: f64 = 0.002;
/// Sample frames the sound reader waits for before it pushes more.
const AUDIO_CHUNK_FRAMES: usize = 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    /// Upright size, the rotation in the container already applied.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub has_audio: bool,
}

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// Seconds from the start of the file.
    pub position: f64,
}

/// ffmpeg and ffprobe are both on PATH. Checked once per process.
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| which::which("ffmpeg").is_ok() && which::which("ffprobe").is_ok())
}

/// Reads size, frame rate, duration and whether there is a sound track.
pub async fn probe(path: &Path) -> Option<VideoInfo> {
    if !available() {
        return None;
    }
    let output = crate::process::async_command("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height,avg_frame_rate,r_frame_rate:stream_side_data=rotation:format=duration", "-of", "json"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_probe(&String::from_utf8_lossy(&output.stdout))
}

fn parse_rate(text: &str) -> Option<f64> {
    let (numer, denom) = text.split_once('/')?;
    let (numer, denom): (f64, f64) = (numer.trim().parse().ok()?, denom.trim().parse().ok()?);
    (numer > 0. && denom > 0.).then(|| numer / denom)
}

fn parse_probe(json: &str) -> Option<VideoInfo> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let streams = value.get("streams")?.as_array()?;
    let video = streams.iter().find(|stream| stream.get("codec_type").and_then(|kind| kind.as_str()) == Some("video"))?;
    let has_audio = streams.iter().any(|stream| stream.get("codec_type").and_then(|kind| kind.as_str()) == Some("audio"));
    let mut width = video.get("width")?.as_u64()? as u32;
    let mut height = video.get("height")?.as_u64()? as u32;
    let rotation = video
        .get("side_data_list")
        .and_then(|list| list.as_array())
        .and_then(|list| list.iter().find_map(|entry| entry.get("rotation").and_then(|rotation| rotation.as_f64())))
        .unwrap_or(0.);
    if (rotation.abs().rem_euclid(180.) - 90.).abs() < 1. {
        std::mem::swap(&mut width, &mut height);
    }
    let fps = ["avg_frame_rate", "r_frame_rate"].iter().find_map(|key| video.get(key).and_then(|rate| rate.as_str()).and_then(parse_rate)).unwrap_or(30.);
    let duration = value.get("format").and_then(|format| format.get("duration")).and_then(|duration| duration.as_str()).and_then(|text| text.parse().ok()).unwrap_or(0.);
    (width > 0 && height > 0).then_some(VideoInfo { width, height, fps: fps.clamp(1., 240.), duration, has_audio })
}

/// The size frames are decoded at: the box on a `scale` display, never larger
/// than the file itself, both sides even as the scaler wants.
pub fn decode_size(info: &VideoInfo, box_width: f32, box_height: f32, scale: f32) -> (u32, u32) {
    let (natural_w, natural_h) = (info.width as f32, info.height as f32);
    let factor = ((box_width * scale) / natural_w).min((box_height * scale) / natural_h).min(1.).max(0.01);
    let even = |value: f32| ((value.round() as u32).max(2) / 2 * 2).max(2);
    (even(natural_w * factor), even(natural_h * factor))
}

enum Audio {
    /// The device is still being opened; the picture waits.
    Pending,
    Sink(Arc<AudioSink>),
    /// No track or no device: wall time paces the picture.
    Wall { running_since: Option<Instant>, elapsed: Duration },
}

struct Clock {
    start: f64,
    audio: Mutex<Audio>,
    paused: AtomicBool,
    muted: AtomicBool,
}

impl Clock {
    fn now(&self) -> Option<f64> {
        let audio = self.audio.lock().ok()?;
        match &*audio {
            Audio::Pending => None,
            Audio::Sink(sink) => Some(self.start + sink.position()),
            Audio::Wall { running_since, elapsed } => Some(self.start + (*elapsed + running_since.map(|since| since.elapsed()).unwrap_or_default()).as_secs_f64()),
        }
    }

    fn set_sink(&self, sink: Arc<AudioSink>) {
        sink.set_muted(self.muted.load(Ordering::Relaxed));
        if self.paused.load(Ordering::Relaxed) {
            sink.pause();
        }
        if let Ok(mut audio) = self.audio.lock() {
            *audio = Audio::Sink(sink);
        }
    }

    fn set_wall(&self) {
        if let Ok(mut audio) = self.audio.lock() {
            let running_since = (!self.paused.load(Ordering::Relaxed)).then(Instant::now);
            *audio = Audio::Wall { running_since, elapsed: Duration::ZERO };
        }
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        let Ok(mut audio) = self.audio.lock() else { return };
        match &mut *audio {
            Audio::Pending => {}
            Audio::Sink(sink) => {
                if paused {
                    sink.pause()
                } else {
                    sink.play()
                }
            }
            Audio::Wall { running_since, elapsed } => {
                if paused {
                    if let Some(since) = running_since.take() {
                        *elapsed += since.elapsed();
                    }
                } else if running_since.is_none() {
                    *running_since = Some(Instant::now());
                }
            }
        }
    }

    fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        if let Ok(audio) = self.audio.lock()
            && let Audio::Sink(sink) = &*audio
        {
            sink.set_muted(muted);
        }
    }
}

/// One run of the decoders from a start position. Dropping it kills both
/// ffmpeg processes and closes the device.
pub struct Playback {
    clock: Arc<Clock>,
    paused: watch::Sender<bool>,
    ended: Arc<AtomicBool>,
    position: Arc<AtomicU64>,
    tasks: Vec<JoinHandle<()>>,
}

impl Playback {
    /// Starts decoding `path` at `start` seconds, `width` by `height` pixels a
    /// frame, and sends frames on `frames` as their time comes. The channel
    /// closes when the file ends or the picture decoder fails.
    #[allow(clippy::too_many_arguments)]
    pub fn start(runtime: &tokio::runtime::Handle, path: &Path, info: &VideoInfo, width: u32, height: u32, start: f64, muted: bool, frames: mpsc::Sender<VideoFrame>) -> Playback {
        let start = start.clamp(0., info.duration.max(0.));
        let clock = Arc::new(Clock { start, audio: Mutex::new(Audio::Pending), paused: AtomicBool::new(false), muted: AtomicBool::new(muted) });
        let (paused, paused_rx) = watch::channel(false);
        let ended = Arc::new(AtomicBool::new(false));
        let position = Arc::new(AtomicU64::new(start.to_bits()));
        let mut tasks = Vec::with_capacity(2);
        if info.has_audio {
            tasks.push(runtime.spawn(run_audio(path.to_path_buf(), start, clock.clone())));
        } else {
            clock.set_wall();
        }
        tasks.push(runtime.spawn(run_video(path.to_path_buf(), start, info.fps, width, height, clock.clone(), paused_rx, frames, position.clone(), ended.clone())));
        Playback { clock, paused, ended, position, tasks }
    }

    pub fn pause(&self) {
        self.clock.set_paused(true);
        let _ = self.paused.send(true);
    }

    pub fn resume(&self) {
        self.clock.set_paused(false);
        let _ = self.paused.send(false);
    }

    pub fn paused(&self) -> bool {
        *self.paused.borrow()
    }

    pub fn set_muted(&self, muted: bool) {
        self.clock.set_muted(muted);
    }

    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }

    /// Seconds into the file of the last frame let through.
    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Acquire))
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_video(path: PathBuf, start: f64, fps: f64, width: u32, height: u32, clock: Arc<Clock>, mut paused: watch::Receiver<bool>, frames: mpsc::Sender<VideoFrame>, position: Arc<AtomicU64>, ended: Arc<AtomicBool>) {
    let spawned = crate::process::async_command("ffmpeg")
        .args(["-v", "error", "-nostdin", "-ss", &format!("{start:.3}"), "-i"])
        .arg(&path)
        .args(["-map", "0:v:0", "-an", "-fps_mode", "cfr", "-r", &format!("{fps:.4}"), "-vf", &format!("scale={width}:{height}"), "-pix_fmt", "bgra", "-f", "rawvideo", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            tracing::error!("video: ffmpeg: {error}");
            ended.store(true, Ordering::Release);
            return;
        }
    };
    let Some(mut stdout) = child.stdout.take() else { return };
    let frame_len = width as usize * height as usize * 4;
    let frame_time = 1. / fps;
    let mut index: u64 = 0;
    loop {
        let mut bgra = vec![0u8; frame_len];
        if stdout.read_exact(&mut bgra).await.is_err() {
            break;
        }
        let pts = start + index as f64 * frame_time;
        index += 1;
        let now = loop {
            // The first frame goes through paused too, so a seek while paused
            // still lands on the picture at the new position.
            if index > 1 && *paused.borrow() && paused.wait_for(|paused| !*paused).await.is_err() {
                return;
            }
            match clock.now() {
                None => tokio::time::sleep(Duration::from_millis(5)).await,
                Some(now) if now + SLACK >= pts => break now,
                Some(now) => tokio::time::sleep(Duration::from_secs_f64((pts - now).min(0.05))).await,
            }
        };
        position.store(pts.to_bits(), Ordering::Release);
        // A frame the sound has already passed is not worth a texture; the
        // decoder is ahead of us, so the next one will be on time.
        if now - pts > frame_time && index > 1 {
            continue;
        }
        match frames.try_send(VideoFrame { width, height, bgra, position: pts }) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => return,
        }
    }
    ended.store(true, Ordering::Release);
}

async fn run_audio(path: PathBuf, start: f64, clock: Arc<Clock>) {
    let Some(sink) = AudioSink::open().await else {
        clock.set_wall();
        return;
    };
    let sink = Arc::new(sink);
    let format = sink.format();
    clock.set_sink(sink.clone());
    let spawned = crate::process::async_command("ffmpeg")
        .args(["-v", "error", "-nostdin", "-ss", &format!("{start:.3}"), "-i"])
        .arg(&path)
        .args(["-map", "0:a:0", "-vn", "-f", "f32le", "-ac", &format.channels.to_string(), "-ar", &format.rate.to_string(), "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            tracing::error!("video: ffmpeg audio: {error}");
            sink.finish();
            return;
        }
    };
    let Some(mut stdout) = child.stdout.take() else { return };
    let channels = format.channels.max(1) as usize;
    let mut bytes = vec![0u8; AUDIO_CHUNK_FRAMES * channels * 4];
    let mut samples = Vec::with_capacity(AUDIO_CHUNK_FRAMES * channels);
    loop {
        while sink.room() < AUDIO_CHUNK_FRAMES {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if stdout.read_exact(&mut bytes).await.is_err() {
            break;
        }
        samples.clear();
        samples.extend(bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])));
        sink.push(&samples);
    }
    sink.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = r#"{"streams":[{"codec_type":"audio","r_frame_rate":"0/0","avg_frame_rate":"0/0"},{"codec_type":"video","width":1280,"height":720,"r_frame_rate":"30/1","avg_frame_rate":"30/1","side_data_list":[{},{"rotation":-90},{}]}],"format":{"duration":"26.760000"}}"#;

    #[test]
    fn a_rotated_clip_is_reported_upright() {
        let info = parse_probe(PROBE).unwrap();
        assert_eq!(info, VideoInfo { width: 720, height: 1280, fps: 30., duration: 26.76, has_audio: true });
    }

    #[test]
    fn a_clip_without_sound_or_rotation_keeps_its_size() {
        let json = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"r_frame_rate":"60/1","avg_frame_rate":"0/0"}],"format":{}}"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(info, VideoInfo { width: 1920, height: 1080, fps: 60., duration: 0., has_audio: false });
    }

    #[test]
    fn a_file_with_no_picture_is_not_a_video() {
        assert!(parse_probe(r#"{"streams":[{"codec_type":"audio"}],"format":{}}"#).is_none());
    }

    /// A one second test pattern with a tone, rendered once by ffmpeg under target/.
    fn fixture() -> Option<PathBuf> {
        if !available() {
            return None;
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-home/video");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("pattern.mp4");
        if !path.exists() {
            let status = std::process::Command::new("ffmpeg")
                .args(["-v", "error", "-nostdin", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x120:rate=30:duration=1", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:v", "mpeg4", "-c:a", "aac", "-shortest", "-f", "mp4"])
                .arg(&path)
                .status()
                .ok()?;
            assert!(status.success(), "ffmpeg could not render the fixture");
        }
        Some(path)
    }

    async fn collect(runtime: &tokio::runtime::Handle, path: &Path, info: &VideoInfo, from: f64) -> (Vec<f64>, bool) {
        let (tx, mut rx) = mpsc::channel(2);
        let playback = Playback::start(runtime, path, info, 80, 60, from, true, tx);
        let mut positions = Vec::new();
        while let Some(frame) = rx.recv().await {
            assert_eq!((frame.width, frame.height, frame.bgra.len()), (80, 60, 80 * 60 * 4));
            positions.push(frame.position);
        }
        (positions, playback.ended())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plays_a_clip_to_the_end_in_order_and_from_a_seek() {
        let Some(path) = fixture() else { return };
        let info = probe(&path).await.expect("probe");
        assert_eq!((info.width, info.height, info.has_audio), (160, 120, true));
        assert!((info.fps - 30.).abs() < 0.01 && (info.duration - 1.).abs() < 0.1, "{info:?}");

        let runtime = tokio::runtime::Handle::current();
        let started = std::time::Instant::now();
        let (positions, ended) = collect(&runtime, &path, &info, 0.).await;
        assert!(ended);
        assert!(positions.len() >= 15, "{} frames", positions.len());
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(positions[0] < 0.01);
        assert!(started.elapsed().as_secs_f64() > 0.8, "frames were not paced");

        let (positions, ended) = collect(&runtime, &path, &info, 0.5).await;
        assert!(ended);
        assert!((positions[0] - 0.5).abs() < 0.01, "first frame at {}", positions[0]);
        assert!(positions.len() <= 16, "{} frames after the seek", positions.len());
    }

    #[test]
    fn frames_decode_at_the_box_on_a_retina_display_but_never_above_the_file() {
        let info = VideoInfo { width: 1280, height: 720, fps: 30., duration: 1., has_audio: false };
        assert_eq!(decode_size(&info, 400., 400., 2.), (800, 450));
        assert_eq!(decode_size(&info, 1600., 900., 2.), (1280, 720));
        let portrait = VideoInfo { width: 720, height: 1280, ..info };
        assert_eq!(decode_size(&portrait, 1000., 500., 1.), (280, 500));
    }
}
