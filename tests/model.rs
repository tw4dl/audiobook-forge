use kokoro_book::model::{MODEL_ARCHIVE_SHA256, MODEL_BUNDLE_NAME, ModelAssets};
use tempfile::tempdir;

#[test]
fn reports_every_missing_required_model_asset() {
    let temp = tempdir().expect("temp dir");
    let error = ModelAssets::from_dir(temp.path()).expect_err("empty model dir must fail");
    let message = error.to_string();

    for required in [
        "model.onnx",
        "voices.bin",
        "tokens.txt",
        "espeak-ng-data",
        "lexicon-us-en.txt",
    ] {
        assert!(
            message.contains(required),
            "missing {required} in {message}"
        );
    }
}

#[test]
fn pins_the_kokoro_v1_bundle_name() {
    assert_eq!(MODEL_BUNDLE_NAME, "kokoro-multi-lang-v1_0");
    assert_eq!(MODEL_ARCHIVE_SHA256.len(), 64);
}

#[test]
fn accepts_only_the_assets_needed_for_english() {
    let temp = tempdir().expect("temp dir");
    for file in [
        "model.onnx",
        "voices.bin",
        "tokens.txt",
        "lexicon-us-en.txt",
    ] {
        std::fs::write(temp.path().join(file), b"fixture").expect("fixture file");
    }
    std::fs::create_dir(temp.path().join("espeak-ng-data")).expect("fixture directory");

    ModelAssets::from_dir(temp.path()).expect("English-only asset layout");
}
