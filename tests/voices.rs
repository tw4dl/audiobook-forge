use kokoro_book::voice::{DEFAULT_VOICE, ENGLISH_VOICES, Voice};

#[test]
fn default_voice_is_the_benchmarked_preset() {
    let voice: Voice = DEFAULT_VOICE.parse().expect("default voice");
    assert_eq!(voice.name(), "af_heart");
    assert_eq!(voice.speaker_id(), 3);
}

#[test]
fn exposes_only_english_presets() {
    assert_eq!(ENGLISH_VOICES.len(), 28);
    assert!(ENGLISH_VOICES.iter().all(|voice| {
        voice.name.starts_with("af_")
            || voice.name.starts_with("am_")
            || voice.name.starts_with("bf_")
            || voice.name.starts_with("bm_")
    }));
}

#[test]
fn rejects_unknown_and_non_english_voices() {
    for name in ["made_up", "ef_dora", "zf_xiaobei"] {
        let error = name.parse::<Voice>().expect_err("voice must fail");
        assert!(error.to_string().contains("unknown English voice"));
    }
}
