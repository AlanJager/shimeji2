//! Pose sounds. PCM was decoded in Rust (`shimeji-packs`). This module copies
//! those samples into an `AVAudioPCMBuffer`. Pack bytes never reach a system
//! audio decoder.
//!
//! Pack sounds follow the saved setting. `SHIMEJI_SOUND` overrides it for this
//! process. `SHIMEJI_DEBUG_TONE=1` plays one generated tone through the same
//! engine and is off in normal runs. The bundled shime has no sounds.

use std::collections::HashMap;

#[cfg(target_os = "macos")]
use objc2::AnyThread;

/// `1` / `true` / `yes` / `on` enables pack sounds.
pub const SOUND_ENV: &str = "SHIMEJI_SOUND";

/// Plays one sine burst at launch, ignoring the mute, to prove the engine.
pub const DEBUG_TONE_ENV: &str = "SHIMEJI_DEBUG_TONE";

/// Decoded integer PCM. Interleaved, little-endian, as the WAV parser left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub pcm: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FloatFrames {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

pub fn env_flag(value: Option<&str>) -> bool {
    matches!(
        value
            .map(|text| text.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// 16-bit mono sine. `amplitude` is 0..1. Used by the debug tone and tests.
pub fn sine_i16(sample_rate: u32, hz: f32, millis: u32, amplitude: f32) -> Vec<u8> {
    let frames = (u64::from(sample_rate) * u64::from(millis) / 1000) as usize;
    let mut pcm = Vec::with_capacity(frames.saturating_mul(2));
    let amp = amplitude.clamp(0.0, 1.0);
    for index in 0..frames {
        let t = index as f32 / sample_rate as f32;
        let sample = (t * hz * std::f32::consts::TAU).sin() * amp;
        let scaled = (sample * 32767.0).round() as i16;
        pcm.extend_from_slice(&scaled.to_le_bytes());
    }
    pcm
}

pub fn debug_tone() -> Clip {
    Clip {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        pcm: sine_i16(44_100, 440.0, 200, 0.2),
    }
}

/// Interleaved integer PCM to per-channel float frames in -1..1.
/// 8-bit is unsigned. 16-bit and 32-bit are signed little-endian. Anything
/// else yields an empty channel list.
pub fn to_float_frames(clip: &Clip, gain: f32) -> FloatFrames {
    let channels = clip.channels.max(1) as usize;
    let width: usize = match clip.bits_per_sample {
        8 => 1,
        16 => 2,
        32 => 4,
        _ => {
            return FloatFrames {
                sample_rate: clip.sample_rate,
                channels: Vec::new(),
            };
        }
    };
    let frame_bytes = width.saturating_mul(channels);
    if frame_bytes == 0 {
        return FloatFrames {
            sample_rate: clip.sample_rate,
            channels: Vec::new(),
        };
    }
    let frames = clip.pcm.len() / frame_bytes;
    let mut out = vec![Vec::with_capacity(frames); channels];
    let gain = gain.clamp(0.0, 1.0);
    for frame in 0..frames {
        for (channel, dest) in out.iter_mut().enumerate() {
            let at = frame * frame_bytes + channel * width;
            let sample = match width {
                1 => (clip.pcm[at] as f32 - 128.0) / 128.0,
                2 => i16::from_le_bytes([clip.pcm[at], clip.pcm[at + 1]]) as f32 / 32768.0,
                _ => {
                    i32::from_le_bytes([
                        clip.pcm[at],
                        clip.pcm[at + 1],
                        clip.pcm[at + 2],
                        clip.pcm[at + 3],
                    ]) as f32
                        / 2147483648.0
                }
            };
            dest.push(sample * gain);
        }
    }
    FloatFrames {
        sample_rate: clip.sample_rate,
        channels: out,
    }
}

/// Pose `Volume`, when it parses. Values above 1 are treated as 0..100 percent.
pub fn gain_from_volume(volume: Option<&str>) -> f32 {
    let Some(text) = volume else {
        return 1.0;
    };
    let Ok(value) = text.trim().parse::<f32>() else {
        return 1.0;
    };
    if value > 1.0 {
        (value / 100.0).clamp(0.0, 1.0)
    } else {
        value.clamp(0.0, 1.0)
    }
}

pub fn clip_aliases(path: &str) -> Vec<String> {
    let mut keys = vec![path.to_string()];
    let trimmed = path.trim().trim_start_matches('/');
    if trimmed != path {
        keys.push(trimmed.to_string());
    }
    if let Some(name) = std::path::Path::new(trimmed).file_name() {
        let name = name.to_string_lossy();
        keys.push(name.to_string());
        if let Some(stem) = std::path::Path::new(name.as_ref()).file_stem() {
            keys.push(stem.to_string_lossy().into_owned());
        }
    }
    keys
}

#[derive(Debug, Default)]
pub struct Library {
    clips: HashMap<String, Clip>,
}

impl Library {
    pub fn insert(&mut self, path: &str, clip: Clip) {
        for key in clip_aliases(path) {
            self.clips.insert(key, clip.clone());
        }
    }

    pub fn get(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name).or_else(|| {
            clip_aliases(name)
                .iter()
                .find_map(|key| self.clips.get(key))
        })
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

/// Pack sounds stay silent unless enabled. The debug tone does not use this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mute {
    pub enabled: bool,
}

impl Mute {
    pub fn allows(self) -> bool {
        self.enabled
    }
}

#[cfg(target_os = "macos")]
pub struct Player {
    engine: objc2::rc::Retained<objc2_avf_audio::AVAudioEngine>,
    node: objc2::rc::Retained<objc2_avf_audio::AVAudioPlayerNode>,
    started: bool,
    scheduled: u64,
}

#[cfg(target_os = "macos")]
impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl Player {
    pub fn new() -> Self {
        use objc2_avf_audio::{AVAudioEngine, AVAudioPlayerNode};
        let engine = unsafe { AVAudioEngine::init(AVAudioEngine::alloc()) };
        let node = unsafe { AVAudioPlayerNode::init(AVAudioPlayerNode::alloc()) };
        Self {
            engine,
            node,
            started: false,
            scheduled: 0,
        }
    }

    pub fn started(&self) -> bool {
        self.started
    }

    pub fn scheduled(&self) -> u64 {
        self.scheduled
    }

    /// Copy `frames` into a float PCM buffer and schedule it. Returns false
    /// when the format or the engine refuses the buffer.
    pub fn play_frames(&mut self, frames: &FloatFrames) -> bool {
        if frames.channels.is_empty() || frames.channels[0].is_empty() || frames.sample_rate == 0 {
            return false;
        }
        let Some(buffer) = pcm_buffer(frames) else {
            return false;
        };
        if !self.ensure_started(frames) {
            return false;
        }
        unsafe {
            self.node
                .scheduleBuffer_completionHandler(&buffer, std::ptr::null_mut());
            self.node.play();
        }
        self.scheduled += 1;
        true
    }

    fn ensure_started(&mut self, frames: &FloatFrames) -> bool {
        if self.started {
            return true;
        }
        use objc2::ClassType;
        use objc2_avf_audio::AVAudioFormat;
        let channels = frames.channels.len() as u32;
        let Some(format) = (unsafe {
            AVAudioFormat::initStandardFormatWithSampleRate_channels(
                AVAudioFormat::alloc(),
                f64::from(frames.sample_rate),
                channels,
            )
        }) else {
            return false;
        };
        let mixer = unsafe { self.engine.mainMixerNode() };
        unsafe {
            self.engine.attachNode(self.node.as_super());
            self.engine
                .connect_to_format(self.node.as_super(), mixer.as_super(), Some(&format));
            if self.engine.startAndReturnError().is_err() {
                return false;
            }
            self.node.play();
        }
        self.started = true;
        true
    }
}

#[cfg(target_os = "macos")]
fn pcm_buffer(
    frames: &FloatFrames,
) -> Option<objc2::rc::Retained<objc2_avf_audio::AVAudioPCMBuffer>> {
    use std::ptr::NonNull;

    use objc2_avf_audio::{AVAudioFormat, AVAudioPCMBuffer};

    let channels = frames.channels.len() as u32;
    let count = frames.channels[0].len() as u32;
    let format = unsafe {
        AVAudioFormat::initStandardFormatWithSampleRate_channels(
            AVAudioFormat::alloc(),
            f64::from(frames.sample_rate),
            channels,
        )?
    };
    let buffer = unsafe {
        AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
            AVAudioPCMBuffer::alloc(),
            &format,
            count,
        )?
    };
    unsafe { buffer.setFrameLength(count) };
    let data = unsafe { buffer.floatChannelData() };
    if data.is_null() {
        return None;
    }
    for (index, channel) in frames.channels.iter().enumerate() {
        let dest: NonNull<f32> = unsafe { *data.add(index) };
        let dest = dest.as_ptr();
        for (frame, sample) in channel.iter().enumerate() {
            unsafe { *dest.add(frame) = *sample };
        }
    }
    Some(buffer)
}
