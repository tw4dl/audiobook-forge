use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn help_describes_the_single_conversion_command() {
    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("EPUB or TXT"))
        .stdout(predicate::str::contains("--voice"))
        .stdout(predicate::str::contains("--pronunciation"))
        .stdout(predicate::str::contains("--output"));
}

#[test]
fn voices_lists_presets_without_loading_a_model() {
    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("voices")
        .assert()
        .success()
        .stdout(predicate::str::contains("af_heart"))
        .stdout(predicate::str::contains("bm_lewis"));
}

#[test]
fn invalid_input_fails_before_any_download() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("book.pdf");
    std::fs::write(&input, b"bad").expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg(&input)
        .env("KOKORO_BOOK_CACHE_DIR", temp.path().join("cache"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "supported input types: .epub and .txt",
        ));

    assert!(!temp.path().join("cache").exists());
}

#[test]
fn invalid_options_fail_before_any_download() {
    for (arguments, expected) in [
        (vec!["--speed", "3"], "speed must be between 0.5 and 2.0"),
        (
            vec!["--chunk-phonemes", "0"],
            "phoneme limit must be greater than zero",
        ),
        (vec!["--voice", "ef_dora"], "unknown English voice"),
        (
            vec!["--pronunciation", "Cormer"],
            "pronunciation must use WORD=IPA",
        ),
        (
            vec!["--output", "book.mp3"],
            "output must use the .wav extension",
        ),
    ] {
        let temp = tempdir().expect("temp dir");
        let input = temp.path().join("book.txt");
        let cache = temp.path().join("cache");
        std::fs::write(&input, "A short test sentence.").expect("fixture");

        Command::cargo_bin("kokoro-book")
            .expect("binary")
            .arg(&input)
            .args(arguments)
            .env("KOKORO_BOOK_CACHE_DIR", &cache)
            .assert()
            .failure()
            .stderr(predicate::str::contains(expected));

        assert!(!cache.exists());
    }
}
