use anyhow::{Result, bail};

const MAX_BACK_REFERENCE_DISTANCE: usize = 2_047;

pub(super) fn decompress(input: &[u8], output_limit: usize) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len().min(output_limit));
    let mut index = 0_usize;
    while index < input.len() {
        let byte = input[index];
        index += 1;
        match byte {
            0 | 9..=0x7f => push_byte(&mut output, byte, output_limit)?,
            1..=8 => {
                let count = usize::from(byte);
                let end = index
                    .checked_add(count)
                    .ok_or_else(|| anyhow::anyhow!("PalmDOC literal length overflows the input"))?;
                let literal = input
                    .get(index..end)
                    .ok_or_else(|| anyhow::anyhow!("PalmDOC literal extends past its record"))?;
                extend(&mut output, literal, output_limit)?;
                index = end;
            }
            0x80..=0xbf => {
                let next = *input.get(index).ok_or_else(|| {
                    anyhow::anyhow!("PalmDOC back-reference is missing its second byte")
                })?;
                index += 1;
                let pair = (u16::from(byte) << 8) | u16::from(next);
                let distance = usize::from((pair >> 3) & 0x07ff);
                let length = usize::from((pair & 0x0007) + 3);
                if distance == 0
                    || distance > MAX_BACK_REFERENCE_DISTANCE
                    || distance > output.len()
                {
                    bail!("PalmDOC back-reference has invalid distance {distance}");
                }
                for _ in 0..length {
                    let source = output.len() - distance;
                    let value = output[source];
                    push_byte(&mut output, value, output_limit)?;
                }
            }
            _ => {
                push_byte(&mut output, b' ', output_limit)?;
                push_byte(&mut output, byte ^ 0x80, output_limit)?;
            }
        }
    }
    Ok(output)
}

pub(super) fn strip_trailing_data(record: &[u8], flags: u32) -> Result<&[u8]> {
    let mut end = record.len();
    for _ in 0..(flags >> 1).count_ones() {
        let length = trailing_variable_width_length(&record[..end])?;
        if length == 0 || length > end {
            bail!("MOBI trailing-data length {length} exceeds its record");
        }
        end -= length;
    }
    if flags & 1 != 0 {
        let last = *record
            .get(end.saturating_sub(1))
            .ok_or_else(|| anyhow::anyhow!("MOBI multibyte trailer is empty"))?;
        let length = usize::from((last & 0b11) + 1);
        if length > end {
            bail!("MOBI multibyte trailer exceeds its record");
        }
        end -= length;
    }
    Ok(&record[..end])
}

fn trailing_variable_width_length(record: &[u8]) -> Result<usize> {
    let mut value = 0_usize;
    let mut shift = 0_u32;
    for byte in record.iter().rev().take(4) {
        value |= usize::from(byte & 0x7f)
            .checked_shl(shift)
            .ok_or_else(|| anyhow::anyhow!("MOBI trailing-data length overflows"))?;
        if byte & 0x80 != 0 {
            return Ok(value);
        }
        shift += 7;
    }
    bail!("MOBI trailing-data length has no start marker")
}

fn extend(output: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<()> {
    let next = output
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| anyhow::anyhow!("PalmDOC output length overflows"))?;
    if next > limit {
        bail!("PalmDOC output exceeds declared text length");
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn push_byte(output: &mut Vec<u8>, byte: u8, limit: usize) -> Result<()> {
    if output.len() >= limit {
        bail!("PalmDOC output exceeds declared text length");
    }
    output.push(byte);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decompress, strip_trailing_data};

    #[test]
    fn expands_literals_spaces_and_overlapping_back_references() {
        let input = [3, b'a', b'b', b'c', 0x80, 0x18, 0xc1];
        let output = decompress(&input, 8).expect("valid PalmDOC");
        assert_eq!(output, b"abcabc A");
    }

    #[test]
    fn rejects_output_expansion_beyond_the_bound() {
        let error = decompress(&[3, b'a', b'b', b'c', 0x80, 0x18], 5).expect_err("expansion bound");
        assert!(error.to_string().contains("declared text length"));
    }

    #[test]
    fn strips_variable_width_and_multibyte_trailers_safely() {
        let record = [b'a', b'b', b'c', 0x00, 0x81];
        let payload = strip_trailing_data(&record, 0b11).expect("valid trailer");
        assert_eq!(payload, b"abc");
    }
}
