use anyhow::{Result, bail};
use audiobook_forge::worker::{ChunkAudio, ChunkWorker, synthesize_with_split_retry};

#[derive(Default)]
struct FailsFirstWorker {
    requests: Vec<String>,
    restarts: usize,
}

impl ChunkWorker for FailsFirstWorker {
    fn synthesize(&mut self, phonemes: &str, _speed: f32) -> Result<ChunkAudio> {
        self.requests.push(phonemes.to_owned());
        if self.requests.len() == 1 {
            bail!("worker exited");
        }
        let sample = match self.requests.len() {
            2 => 2.0,
            3 => 3.0,
            count => panic!("unexpected request count {count}"),
        };
        Ok(ChunkAudio::from_samples(vec![sample]))
    }

    fn restart(&mut self) -> Result<()> {
        self.restarts += 1;
        Ok(())
    }
}

#[test]
fn restarts_then_splits_a_failed_chunk_once() {
    let mut worker = FailsFirstWorker::default();

    let audio =
        synthesize_with_split_retry(&mut worker, "aa bb cc dd", 1.0).expect("split retry succeeds");

    assert_eq!(worker.restarts, 1);
    assert_eq!(worker.requests, ["aa bb cc dd", "aa bb", "cc dd"]);
    assert_eq!(audio.len(), 2);
    assert_eq!(audio[0].samples, [2.0]);
    assert_eq!(audio[1].samples, [3.0]);
}

struct AlwaysFailsWorker {
    requests: usize,
    restarts: usize,
}

impl ChunkWorker for AlwaysFailsWorker {
    fn synthesize(&mut self, _phonemes: &str, _speed: f32) -> Result<ChunkAudio> {
        self.requests += 1;
        bail!("still failed")
    }

    fn restart(&mut self) -> Result<()> {
        self.restarts += 1;
        Ok(())
    }
}

#[test]
fn does_not_recursively_split_a_failed_retry() {
    let mut worker = AlwaysFailsWorker {
        requests: 0,
        restarts: 0,
    };

    let error = synthesize_with_split_retry(&mut worker, "aa bb cc dd", 1.0)
        .expect_err("second failure must escape");

    assert!(error.to_string().contains("split retry failed"));
    assert_eq!(worker.restarts, 1);
    assert_eq!(worker.requests, 2);
}
