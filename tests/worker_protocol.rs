use std::io::Cursor;

use audiobook_forge::worker::{
    WorkerRequest, WorkerResponse, WorkerStats, read_request, read_response, write_request,
    write_response,
};

#[test]
fn request_frame_round_trips_unicode_phonemes() {
    let request = WorkerRequest {
        phonemes: "ðə wɝld".to_owned(),
        speed: 1.125,
    };
    let mut bytes = Vec::new();

    write_request(&mut bytes, &request).expect("write request");
    let decoded = read_request(&mut Cursor::new(bytes))
        .expect("read request")
        .expect("request frame");

    assert_eq!(decoded, request);
}

#[test]
fn audio_response_frame_round_trips_pcm_and_memory_stats() {
    let response = WorkerResponse::Audio {
        samples: vec![-0.25, 0.0, 0.5],
        synthesis_seconds: 0.125,
        stats: WorkerStats {
            active_bytes: 10,
            cached_bytes: 0,
            peak_bytes: 40,
        },
    };
    let mut bytes = Vec::new();

    write_response(&mut bytes, &response).expect("write response");
    let decoded = read_response(&mut Cursor::new(bytes)).expect("read response");

    assert_eq!(decoded, response);
}

#[test]
fn worker_error_frame_preserves_the_failure_message() {
    let response = WorkerResponse::Error {
        message: "cache remained above 1 MiB".to_owned(),
    };
    let mut bytes = Vec::new();

    write_response(&mut bytes, &response).expect("write response");

    assert_eq!(
        read_response(&mut Cursor::new(bytes)).expect("read response"),
        response
    );
}

#[test]
fn ready_frame_reports_model_load_and_baseline_memory() {
    let response = WorkerResponse::Ready {
        model_load_seconds: 0.144,
        stats: WorkerStats {
            active_bytes: 312 * 1_024 * 1_024,
            cached_bytes: 0,
            peak_bytes: 313 * 1_024 * 1_024,
        },
    };
    let mut bytes = Vec::new();

    write_response(&mut bytes, &response).expect("write ready response");

    assert_eq!(
        read_response(&mut Cursor::new(bytes)).expect("read ready response"),
        response
    );
}
