//! The sound half of video playback. ffmpeg decodes the track to interleaved
//! f32 at the device's own rate and channel count, the reader pushes it into
//! a queue, and the device callback drains the queue. The number of sample
//! frames the device has pulled is the playback clock the picture is paced
//! against, so lips and sound stay together whatever the pipe buffered.
//!
//! The cpal stream lives on its own thread: `Stream` is not `Send` on every
//! backend, and nothing about it belongs on the runtime.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

/// Sound queued ahead of the device. A third of a second rides out a
/// scheduling hiccup without adding audible latency to a seek.
const QUEUE_SECONDS: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub rate: u32,
    pub channels: u16,
}

struct Shared {
    queue: Mutex<VecDeque<f32>>,
    /// Sample frames (one sample per channel) the device has taken.
    consumed: AtomicU64,
    muted: AtomicBool,
    /// The reader hit the end of the track; once the queue drains the clock
    /// keeps going on wall time so a picture that outlasts its sound still plays out.
    eof: AtomicBool,
    drained_at: Mutex<Option<(u64, Instant)>>,
    channels: usize,
    rate: u32,
}

enum Command {
    Play,
    Pause,
}

pub struct AudioSink {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
    format: AudioFormat,
}

impl AudioSink {
    /// Opens the default output device on its own thread. None when there is
    /// no device or it refuses every format, in which case the caller paces
    /// the picture on wall time instead.
    pub async fn open() -> Option<AudioSink> {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Option<(AudioFormat, Arc<Shared>)>>();
        let (commands, command_rx) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("messages-audio".into())
            .spawn(move || {
                let opened = open_stream();
                let Some((stream, format, shared)) = opened else {
                    let _ = ready_tx.send(None);
                    return;
                };
                if stream.play().is_err() {
                    let _ = ready_tx.send(None);
                    return;
                }
                let _ = ready_tx.send(Some((format, shared)));
                // The stream lives as long as this loop; the sender dropping ends both.
                while let Ok(command) = command_rx.recv() {
                    let result = match command {
                        Command::Play => stream.play(),
                        Command::Pause => stream.pause(),
                    };
                    if let Err(error) = result {
                        tracing::warn!("audio: {error}");
                    }
                }
            })
            .ok()?;
        let (format, shared) = ready_rx.await.ok()??;
        Some(AudioSink { shared, commands, format })
    }

    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Whole sample frames the queue can take right now; zero means wait.
    pub fn room(&self) -> usize {
        let cap = (self.format.rate as f64 * QUEUE_SECONDS) as usize * self.shared.channels;
        let queued = self.shared.queue.lock().map(|queue| queue.len()).unwrap_or(cap);
        cap.saturating_sub(queued) / self.shared.channels
    }

    pub fn push(&self, samples: &[f32]) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.extend(samples.iter().copied());
        }
    }

    pub fn finish(&self) {
        self.shared.eof.store(true, Ordering::Release);
    }

    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    /// Seconds of sound the device has played since the sink opened.
    pub fn position(&self) -> f64 {
        let shared = &self.shared;
        let consumed = shared.consumed.load(Ordering::Acquire);
        let base = consumed as f64 / shared.rate as f64;
        if !shared.eof.load(Ordering::Acquire) {
            return base;
        }
        let empty = shared.queue.lock().map(|queue| queue.is_empty()).unwrap_or(true);
        let mut drained = match shared.drained_at.lock() {
            Ok(guard) => guard,
            Err(_) => return base,
        };
        match (*drained, empty) {
            (Some((at, since)), _) if at == consumed => at as f64 / shared.rate as f64 + since.elapsed().as_secs_f64(),
            (_, true) => {
                *drained = Some((consumed, Instant::now()));
                base
            }
            _ => base,
        }
    }
}

type Opened = (cpal::Stream, AudioFormat, Arc<Shared>);

fn open_stream() -> Option<Opened> {
    let device = cpal::default_host().default_output_device()?;
    let supported = match device.default_output_config() {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("audio: no output config: {error}");
            return None;
        }
    };
    let format = AudioFormat { rate: supported.sample_rate(), channels: supported.channels() };
    let shared = Arc::new(Shared {
        queue: Mutex::new(VecDeque::new()),
        consumed: AtomicU64::new(0),
        muted: AtomicBool::new(false),
        eof: AtomicBool::new(false),
        drained_at: Mutex::new(None),
        channels: format.channels.max(1) as usize,
        rate: format.rate.max(1),
    });
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, shared.clone()),
        cpal::SampleFormat::I16 => build::<i16>(&device, config, shared.clone()),
        cpal::SampleFormat::U16 => build::<u16>(&device, config, shared.clone()),
        cpal::SampleFormat::I32 => build::<i32>(&device, config, shared.clone()),
        cpal::SampleFormat::F64 => build::<f64>(&device, config, shared.clone()),
        other => {
            tracing::warn!("audio: unsupported sample format {other:?}");
            return None;
        }
    }?;
    Some((stream, format, shared))
}

fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: cpal::StreamConfig, shared: Arc<Shared>) -> Option<cpal::Stream> {
    let callback = move |out: &mut [T], _: &cpal::OutputCallbackInfo| fill(out, &shared);
    match device.build_output_stream(config, callback, |error| tracing::warn!("audio stream: {error}"), None) {
        Ok(stream) => Some(stream),
        Err(error) => {
            tracing::warn!("audio: {error}");
            None
        }
    }
}

/// Runs on the device thread. A contended lock plays silence for that
/// callback rather than blocking the device; what it skips is still queued.
fn fill<T: SizedSample + FromSample<f32>>(out: &mut [T], shared: &Shared) {
    let silence = T::from_sample(0.0f32);
    let Ok(mut queue) = shared.queue.try_lock() else {
        out.fill(silence);
        return;
    };
    let muted = shared.muted.load(Ordering::Relaxed);
    let take = queue.len().min(out.len()) / shared.channels * shared.channels;
    for (slot, sample) in out.iter_mut().zip(queue.drain(..take)) {
        *slot = if muted { silence } else { T::from_sample(sample) };
    }
    out[take..].fill(silence);
    shared.consumed.fetch_add((take / shared.channels) as u64, Ordering::AcqRel);
}
