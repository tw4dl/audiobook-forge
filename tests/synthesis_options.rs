use kokoro_book::tts::validate_settings;

#[test]
fn accepts_valid_synthesis_settings() {
    validate_settings(1.0, 450, 2).expect("valid settings");
}

#[test]
fn rejects_invalid_synthesis_settings() {
    for (speed, chunk_chars, threads, expected) in [
        (0.49, 450, 2, "speed must be between 0.5 and 2.0"),
        (2.01, 450, 2, "speed must be between 0.5 and 2.0"),
        (f32::NAN, 450, 2, "speed must be between 0.5 and 2.0"),
        (1.0, 0, 2, "chunk size must be greater than zero"),
        (1.0, 450, 0, "threads must be greater than zero"),
    ] {
        let error =
            validate_settings(speed, chunk_chars, threads).expect_err("invalid settings must fail");
        assert_eq!(error.to_string(), expected);
    }
}
