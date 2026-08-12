use audiobook_forge::tts::{DEFAULT_MAX_PHONEMES, validate_settings};

#[test]
fn defaults_to_the_safest_benchmarked_phoneme_limit() {
    assert_eq!(DEFAULT_MAX_PHONEMES, 200);
}

#[test]
fn accepts_valid_synthesis_settings() {
    validate_settings(1.0, 250).expect("valid settings");
}

#[test]
fn rejects_invalid_synthesis_settings() {
    for (speed, max_phonemes, expected) in [
        (0.49, 250, "speed must be between 0.5 and 2.0"),
        (2.01, 250, "speed must be between 0.5 and 2.0"),
        (f32::NAN, 250, "speed must be between 0.5 and 2.0"),
        (1.0, 0, "phoneme limit must be greater than zero"),
        (1.0, 511, "phoneme limit cannot exceed 510"),
    ] {
        let error = validate_settings(speed, max_phonemes).expect_err("invalid settings must fail");
        assert_eq!(error.to_string(), expected);
    }
}
