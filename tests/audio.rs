use hound::WavReader;
use kokoro_book::audio::{SAMPLE_RATE, StreamingWav};
use tempfile::tempdir;

#[test]
fn streams_chunks_and_silence_into_one_pcm_wav() {
    let temp = tempdir().expect("temp dir");
    let output = temp.path().join("book.wav");
    let mut wav = StreamingWav::create(&output).expect("create WAV");

    wav.write_chunk(&[-0.5, 0.0, 0.5]).expect("first PCM");
    wav.write_silence_ms(1).expect("silence");
    wav.write_chunk(&[0.25]).expect("second PCM");
    let report = wav.finish().expect("finish WAV");

    let reader = WavReader::open(&output).expect("read WAV");
    assert_eq!(reader.spec().sample_rate, SAMPLE_RATE);
    assert_eq!(reader.spec().bits_per_sample, 16);
    assert_eq!(reader.len(), 3 + SAMPLE_RATE / 1_000 + 1);
    assert_eq!(report.samples, u64::from(reader.len()));
    assert!(report.peak_amplitude <= 0.5);
}

#[test]
fn rejects_invalid_or_clipping_pcm_before_persisting() {
    for sample in [f32::NAN, f32::INFINITY, 1.0, -1.0, 1.001, -1.001] {
        let temp = tempdir().expect("temp dir");
        let output = temp.path().join("book.wav");
        let mut wav = StreamingWav::create(&output).expect("create WAV");

        let error = wav
            .write_chunk(&[sample])
            .expect_err("unsafe PCM must fail");

        assert!(error.to_string().contains("invalid PCM sample"));
        assert!(!output.exists());
    }
}
