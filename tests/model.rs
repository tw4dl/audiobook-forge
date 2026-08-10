use kokoro_book::model::{MODEL_BUNDLE_NAME, MODEL_REVISION, MODEL_SHA256, ModelAssets};
use kokoro_book::voice::Voice;
use tempfile::tempdir;

#[test]
fn reports_every_missing_required_model_asset() {
    let temp = tempdir().expect("temp dir");
    let voice: Voice = "af_heart".parse().expect("voice");
    let error = ModelAssets::from_dir(temp.path(), voice).expect_err("empty model dir must fail");
    let message = error.to_string();

    for required in ["model_q8f16.onnx", "af_heart.bin"] {
        assert!(
            message.contains(required),
            "missing {required} in {message}"
        );
    }
}

#[test]
fn pins_the_kokoro_v1_bundle_name() {
    assert_eq!(MODEL_BUNDLE_NAME, "Kokoro-82M-v1.0-ONNX-1939ad2a-q8f16");
    assert_eq!(MODEL_REVISION.len(), 40);
    assert_eq!(MODEL_SHA256.len(), 64);
}

#[test]
fn accepts_only_the_model_and_selected_voice() {
    let temp = tempdir().expect("temp dir");
    let voice: Voice = "af_heart".parse().expect("voice");
    std::fs::write(temp.path().join("model_q8f16.onnx"), b"fixture").expect("model fixture");
    std::fs::create_dir(temp.path().join("voices")).expect("voices directory");
    std::fs::write(temp.path().join("voices/af_heart.bin"), b"fixture").expect("voice fixture");

    ModelAssets::from_dir(temp.path(), voice).expect("lean asset layout");
}
