use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[allow(dead_code)]
#[path = "support/structured_epub.rs"]
mod structured_epub;

#[allow(dead_code)]
#[path = "support/pdf_fixture.rs"]
mod pdf_fixture;

#[test]
fn help_describes_conversion_and_inspection() {
    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or TXT",
        ))
        .stdout(predicate::str::contains("inspect"))
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
fn inspect_txt_reports_semantic_sections_without_loading_a_model() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("tiny-book.txt");
    let cache = temp.path().join("cache");
    std::fs::write(
        &input,
        "CHAPTER ONE\nFirst paragraph.\n\nCHAPTER TWO\nSecond paragraph here.\n",
    )
    .expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .env("KOKORO_BOOK_CACHE_DIR", &cache)
        .assert()
        .success()
        .stdout(predicate::str::contains("Title: tiny-book"))
        .stdout(predicate::str::contains("Format: TXT"))
        .stdout(predicate::str::contains("Chapter One"))
        .stdout(predicate::str::contains("Chapter Two"))
        .stdout(predicate::str::contains("Narrated words:"));

    assert!(!cache.exists());
}

#[test]
fn inspect_epub_reports_book_metadata_and_navigation_counts() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("structured.epub");
    structured_epub::write_structured_epub3(&input);

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .assert()
        .success()
        .stdout(predicate::str::contains("Format: EPUB 3.0"))
        .stdout(predicate::str::contains(
            "Authors: Ada Reader; Grace Listener",
        ))
        .stdout(predicate::str::contains("Language: en-US"))
        .stdout(predicate::str::contains(
            "Cover: /EPUB/cover.jpg (image/jpeg)",
        ))
        .stdout(predicate::str::contains("Pages: 2"));
}

#[test]
fn inspect_pdf_reports_metadata_pages_and_bookmarks_without_loading_a_model() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("bookmarked.pdf");
    let cache = temp.path().join("cache");
    pdf_fixture::write_pdf_with_bookmarks(&input);

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .arg("--tree")
        .env("KOKORO_BOOK_CACHE_DIR", &cache)
        .assert()
        .success()
        .stdout(predicate::str::contains("Title: Public Domain Sample"))
        .stdout(predicate::str::contains("Format: PDF 1.7"))
        .stdout(predicate::str::contains("Author: Example Author"))
        .stdout(predicate::str::contains("Pages: 3"))
        .stdout(predicate::str::contains(
            "Public Domain Sample\n  Chapter One\n    A Closer Look\n  Chapter Two",
        ));

    assert!(!cache.exists());
}

#[test]
fn inspect_tree_nests_txt_chapters_under_parts() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("nested.txt");
    std::fs::write(
        &input,
        "PART I\n\nCHAPTER ONE\nFirst paragraph.\n\nCHAPTER TWO\nSecond paragraph.\n\nPART II\n\nCHAPTER THREE\nThird paragraph.\n",
    )
    .expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .arg("--tree")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "nested\n  Part I\n    Chapter One\n    Chapter Two\n  Part II\n    Chapter Three",
        ));
}

#[test]
fn inspect_tree_maps_markdown_heading_levels() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("structured.md");
    std::fs::write(
        &input,
        "# Part I\n\n## Chapter 1\n\n### Why This Matters\n\nUseful prose.\n\n## Chapter 2\n\nMore prose.\n",
    )
    .expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .arg("--tree")
        .assert()
        .success()
        .stdout(predicate::str::contains("Format: Markdown"))
        .stdout(predicate::str::contains(
            "structured\n  Part I\n    Chapter 1\n      Why This Matters\n    Chapter 2",
        ));
}

#[test]
fn inspect_tree_maps_html_heading_levels() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("structured.html");
    std::fs::write(
        &input,
        "<html><body><h1>Part I</h1><section><h2>Chapter One</h2><h3>Details</h3><p>Hello &amp; goodbye.</p></section><h2>Chapter Two</h2><p>The end.</p></body></html>",
    )
    .expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .arg("--tree")
        .assert()
        .success()
        .stdout(predicate::str::contains("Format: HTML"))
        .stdout(predicate::str::contains(
            "structured\n  Part I\n    Chapter One\n      Details\n    Chapter Two",
        ));
}

#[test]
fn inspect_warns_when_txt_has_no_detected_sections() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("plain.txt");
    std::fs::write(&input, "Only unstructured prose.").expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "WARN: TXT has no detected semantic sections; treating the document as one section",
        ))
        .stdout(predicate::str::contains("00  Body  3 words"))
        .stdout(predicate::str::contains("Warnings: 1"));
}

#[test]
fn conversion_reports_structural_warnings_before_synthesis() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("plain.txt");
    let cache = temp.path().join("cache");
    std::fs::write(&input, "Only unstructured prose.").expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg(&input)
        .args(["--speed", "3"])
        .env("KOKORO_BOOK_CACHE_DIR", &cache)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "WARN: TXT has no detected semantic sections; treating the document as one section\nerror: speed must be between 0.5 and 2.0",
        ));

    assert!(!cache.exists());
}

#[test]
fn inspect_sanitizes_terminal_control_characters() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("bad\u{1b}[31m.txt");
    std::fs::write(&input, "CHAPTER ONE\nSafe prose.").expect("fixture");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("inspect")
        .arg(&input)
        .arg("--tree")
        .assert()
        .success()
        .stdout(predicate::str::contains("Title: bad\u{fffd}[31m"))
        .stdout(predicate::str::contains('\u{1b}').not());
}

#[test]
fn errors_sanitize_terminal_control_characters() {
    let temp = tempdir().expect("temp dir");
    let input = temp.path().join("missing\u{1b}[31m.txt");

    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg(&input)
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing\u{fffd}[31m.txt"))
        .stderr(predicate::str::contains('\u{1b}').not());
}

#[test]
fn argument_errors_sanitize_bidirectional_controls() {
    Command::cargo_bin("kokoro-book")
        .expect("binary")
        .arg("--bad\u{202e}option")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--bad\u{fffd}option"))
        .stderr(predicate::str::contains('\u{202e}').not());
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
        .stderr(predicate::str::contains("failed to parse PDF"));

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
