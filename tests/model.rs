use kokoro_book::model::{MODEL_BUNDLE_NAME, MODEL_REVISION, MODEL_SHA256, ModelAssets};
use kokoro_book::voice::Voice;
use tempfile::tempdir;

#[test]
fn reports_every_missing_required_model_asset() {
    let temp = tempdir().expect("temp dir");
    let voice: Voice = "af_heart".parse().expect("voice");
    let error = ModelAssets::from_dir(temp.path(), voice).expect_err("empty model dir must fail");
    let message = error.to_string();

    for required in ["kokoro-v1_0.safetensors", "af_heart.safetensors"] {
        assert!(
            message.contains(required),
            "missing {required} in {message}"
        );
    }
}

#[test]
fn pins_the_kokoro_v1_bundle_name() {
    assert_eq!(MODEL_BUNDLE_NAME, "Kokoro-82M-bf16-a71e4d38");
    assert_eq!(MODEL_REVISION, "a71e4d38b236d968966a2002c4c895dbd12b1c3c");
    assert_eq!(
        MODEL_SHA256,
        "4e9ecdf03b8b6cf906070390237feda473dc13327cb8d56a43deaa374c02acd8"
    );
}

#[test]
fn accepts_only_the_model_and_selected_voice() {
    let temp = tempdir().expect("temp dir");
    let voice: Voice = "af_heart".parse().expect("voice");
    std::fs::write(temp.path().join("kokoro-v1_0.safetensors"), b"fixture").expect("model fixture");
    std::fs::create_dir(temp.path().join("voices")).expect("voices directory");
    std::fs::write(temp.path().join("voices/af_heart.safetensors"), b"fixture")
        .expect("voice fixture");

    ModelAssets::from_dir(temp.path(), voice).expect("lean asset layout");
}
