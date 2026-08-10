//! Binary request and response framing for the worker pipe.

use std::io::{self, Cursor, Read, Write};

use anyhow::{Context, Result, bail};

use super::{WorkerRequest, WorkerResponse, WorkerStats};

const REQUEST_MAGIC: [u8; 4] = *b"KBRQ";
const RESPONSE_MAGIC: [u8; 4] = *b"KBRS";
const PROTOCOL_VERSION: u8 = 1;
const AUDIO_TAG: u8 = 0;
const ERROR_TAG: u8 = 1;
const READY_TAG: u8 = 2;
const MAX_FRAME_BYTES: usize = 128 * 1_024 * 1_024;

/// Write one length-prefixed request frame.
///
/// # Errors
///
/// Returns an I/O or size error.
pub fn write_request<W: Write>(writer: &mut W, request: &WorkerRequest) -> Result<()> {
    let phoneme_len =
        u32::try_from(request.phonemes.len()).context("phoneme request is too large")?;
    let mut payload = Vec::with_capacity(13 + request.phonemes.len());
    payload.extend_from_slice(&REQUEST_MAGIC);
    payload.push(PROTOCOL_VERSION);
    payload.extend_from_slice(&request.speed.to_le_bytes());
    payload.extend_from_slice(&phoneme_len.to_le_bytes());
    payload.extend_from_slice(request.phonemes.as_bytes());
    write_frame(writer, &payload)
}

/// Read one request frame, or `None` after a clean end of input.
///
/// # Errors
///
/// Returns an error for malformed, oversized, partial, or invalid UTF-8 data.
pub fn read_request<R: Read>(reader: &mut R) -> Result<Option<WorkerRequest>> {
    let Some(payload) = read_frame(reader)? else {
        return Ok(None);
    };
    let mut cursor = Cursor::new(payload.as_slice());
    expect_magic(&mut cursor, REQUEST_MAGIC)?;
    expect_version(&mut cursor)?;
    let speed = read_f32(&mut cursor)?;
    let phoneme_len = read_u32(&mut cursor)? as usize;
    let phonemes = read_utf8(&mut cursor, phoneme_len)?;
    expect_end(&cursor, payload.len())?;
    Ok(Some(WorkerRequest { phonemes, speed }))
}

/// Write one length-prefixed worker response frame.
///
/// # Errors
///
/// Returns an I/O or size error.
pub fn write_response<W: Write>(writer: &mut W, response: &WorkerResponse) -> Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&RESPONSE_MAGIC);
    payload.push(PROTOCOL_VERSION);
    match response {
        WorkerResponse::Ready {
            model_load_seconds,
            stats,
        } => {
            payload.push(READY_TAG);
            payload.extend_from_slice(&model_load_seconds.to_le_bytes());
            write_stats(&mut payload, *stats);
        }
        WorkerResponse::Audio {
            samples,
            synthesis_seconds,
            stats,
        } => {
            payload.push(AUDIO_TAG);
            payload.extend_from_slice(&synthesis_seconds.to_le_bytes());
            write_stats(&mut payload, *stats);
            let sample_count =
                u64::try_from(samples.len()).context("audio response is too large")?;
            payload.extend_from_slice(&sample_count.to_le_bytes());
            for sample in samples {
                payload.extend_from_slice(&sample.to_le_bytes());
            }
        }
        WorkerResponse::Error { message } => {
            payload.push(ERROR_TAG);
            let message_len = u32::try_from(message.len()).context("worker error is too large")?;
            payload.extend_from_slice(&message_len.to_le_bytes());
            payload.extend_from_slice(message.as_bytes());
        }
    }
    write_frame(writer, &payload)
}

/// Read one required worker response frame.
///
/// # Errors
///
/// Returns an error for end of input, malformed data, or an oversized frame.
pub fn read_response<R: Read>(reader: &mut R) -> Result<WorkerResponse> {
    let payload = read_frame(reader)?.context("worker exited before sending a response")?;
    let mut cursor = Cursor::new(payload.as_slice());
    expect_magic(&mut cursor, RESPONSE_MAGIC)?;
    expect_version(&mut cursor)?;
    let tag = read_u8(&mut cursor)?;
    let response = match tag {
        READY_TAG => WorkerResponse::Ready {
            model_load_seconds: read_f64(&mut cursor)?,
            stats: read_stats(&mut cursor)?,
        },
        AUDIO_TAG => {
            let synthesis_seconds = read_f64(&mut cursor)?;
            let stats = read_stats(&mut cursor)?;
            let sample_count = usize::try_from(read_u64(&mut cursor)?)
                .context("audio sample count does not fit this platform")?;
            let byte_count = sample_count
                .checked_mul(size_of::<f32>())
                .context("audio sample count overflow")?;
            let position = usize::try_from(cursor.position())
                .context("worker frame position does not fit this platform")?;
            let remaining = payload.len().saturating_sub(position);
            if remaining != byte_count {
                bail!("worker audio frame length does not match its sample count");
            }
            let mut samples = Vec::with_capacity(sample_count);
            for _ in 0..sample_count {
                samples.push(read_f32(&mut cursor)?);
            }
            WorkerResponse::Audio {
                samples,
                synthesis_seconds,
                stats,
            }
        }
        ERROR_TAG => {
            let message_len = read_u32(&mut cursor)? as usize;
            WorkerResponse::Error {
                message: read_utf8(&mut cursor, message_len)?,
            }
        }
        other => bail!("unknown worker response tag {other}"),
    };
    expect_end(&cursor, payload.len())?;
    Ok(response)
}

fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME_BYTES {
        bail!("worker frame exceeds {MAX_FRAME_BYTES} bytes");
    }
    let payload_len = u32::try_from(payload.len()).context("worker frame is too large")?;
    writer.write_all(&payload_len.to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

fn write_stats(payload: &mut Vec<u8>, stats: WorkerStats) {
    payload.extend_from_slice(&stats.active_bytes.to_le_bytes());
    payload.extend_from_slice(&stats.cached_bytes.to_le_bytes());
    payload.extend_from_slice(&stats.peak_bytes.to_le_bytes());
}

fn read_stats(cursor: &mut Cursor<&[u8]>) -> Result<WorkerStats> {
    Ok(WorkerStats {
        active_bytes: read_u64(cursor)?,
        cached_bytes: read_u64(cursor)?,
        peak_bytes: read_u64(cursor)?,
    })
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut length = [0_u8; 4];
    match reader.read(&mut length[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::Interrupted => return read_frame(reader),
        Err(error) => return Err(error.into()),
    }
    reader
        .read_exact(&mut length[1..])
        .context("partial worker frame length")?;
    let payload_len = u32::from_le_bytes(length) as usize;
    if payload_len > MAX_FRAME_BYTES {
        bail!("worker frame exceeds {MAX_FRAME_BYTES} bytes");
    }
    let mut payload = vec![0_u8; payload_len];
    reader
        .read_exact(&mut payload)
        .context("partial worker frame payload")?;
    Ok(Some(payload))
}

fn expect_magic(cursor: &mut Cursor<&[u8]>, expected: [u8; 4]) -> Result<()> {
    let mut actual = [0_u8; 4];
    cursor.read_exact(&mut actual)?;
    if actual != expected {
        bail!("invalid worker frame magic");
    }
    Ok(())
}

fn expect_version(cursor: &mut Cursor<&[u8]>) -> Result<()> {
    let version = read_u8(cursor)?;
    if version != PROTOCOL_VERSION {
        bail!("unsupported worker protocol version {version}");
    }
    Ok(())
}

fn expect_end(cursor: &Cursor<&[u8]>, payload_len: usize) -> Result<()> {
    let position = usize::try_from(cursor.position())
        .context("worker frame position does not fit this platform")?;
    if position != payload_len {
        bail!("worker frame has trailing data");
    }
    Ok(())
}

fn read_utf8(cursor: &mut Cursor<&[u8]>, length: usize) -> Result<String> {
    let position = usize::try_from(cursor.position())
        .context("worker frame position does not fit this platform")?;
    let remaining = cursor.get_ref().len().saturating_sub(position);
    if length > remaining {
        bail!("worker frame string length exceeds payload");
    }
    let mut bytes = vec![0_u8; length];
    cursor.read_exact(&mut bytes)?;
    String::from_utf8(bytes).context("worker frame contains invalid UTF-8")
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8> {
    let mut bytes = [0_u8; 1];
    cursor.read_exact(&mut bytes)?;
    Ok(bytes[0])
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    cursor.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64> {
    let mut bytes = [0_u8; 8];
    cursor.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_f32(cursor: &mut Cursor<&[u8]>) -> Result<f32> {
    let mut bytes = [0_u8; 4];
    cursor.read_exact(&mut bytes)?;
    Ok(f32::from_le_bytes(bytes))
}

fn read_f64(cursor: &mut Cursor<&[u8]>) -> Result<f64> {
    let mut bytes = [0_u8; 8];
    cursor.read_exact(&mut bytes)?;
    Ok(f64::from_le_bytes(bytes))
}
