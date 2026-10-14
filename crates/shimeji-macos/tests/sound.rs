//! PCM conversion. Playback stays behind the debug-tone env var.

use shimeji_macos::platform::sound::{
    Clip, Library, Mute, clip_aliases, debug_tone, env_flag, gain_from_volume, sine_i16,
    to_float_frames,
};

#[test]
fn sine_is_200ms_of_16bit_mono() {
    let pcm = sine_i16(44_100, 440.0, 200, 0.2);
    assert_eq!(pcm.len(), 44_100 * 200 / 1000 * 2);
    let tone = debug_tone();
    assert_eq!(tone.pcm, pcm);
    assert_eq!(tone.channels, 1);
    assert_eq!(tone.sample_rate, 44_100);
}

#[test]
fn mute_defaults_off_and_the_env_stub_is_explicit() {
    assert!(!Mute::default().allows());
    assert!(Mute { enabled: true }.allows());
    assert!(env_flag(Some("1")));
    assert!(env_flag(Some("true")));
    assert!(!env_flag(None));
    assert!(!env_flag(Some("no")));
}

#[test]
fn gain_treats_values_above_one_as_percent() {
    assert_eq!(gain_from_volume(None), 1.0);
    assert_eq!(gain_from_volume(Some("0.5")), 0.5);
    assert_eq!(gain_from_volume(Some("50")), 0.5);
    assert_eq!(gain_from_volume(Some("nope")), 1.0);
    assert_eq!(gain_from_volume(Some("200")), 1.0);
}

#[test]
fn integer_pcm_becomes_float_frames_and_aliases_find_the_clip() {
    let clip = Clip {
        channels: 1,
        sample_rate: 8,
        bits_per_sample: 16,
        pcm: 0i16.to_le_bytes().to_vec(),
    };
    let frames = to_float_frames(&clip, 1.0);
    assert_eq!(frames.channels.len(), 1);
    assert_eq!(frames.channels[0], vec![0.0]);

    let odd = Clip {
        channels: 1,
        sample_rate: 8,
        bits_per_sample: 24,
        pcm: vec![0, 0, 0],
    };
    assert!(to_float_frames(&odd, 1.0).channels.is_empty());

    let mut library = Library::default();
    library.insert("sounds/meow.wav", clip);
    assert!(library.get("meow").is_some());
    assert!(library.get("sounds/meow.wav").is_some());
    assert!(library.get("nope").is_none());
    assert!(clip_aliases("sounds/meow.wav").contains(&"meow".to_string()));
}
