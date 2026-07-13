//! Deterministic host-side `PFG-LZ/1` feasibility codec.
//!
//! This is an original Packforge stream grammar used only to measure whether a
//! future compact native decoder has a credible payload budget. It is not an
//! executable-v2 codec and is intentionally not accepted by the runtime.
//!
//! The authenticated wrapper supplies the decoded length. A stream has no
//! terminator and must be consumed exactly:
//!
//! - `0x00..=0x7f`: a literal run of `tag + 1` bytes;
//! - `0x80..=0xbf`: a copy of `3 + (tag & 0x3f)` bytes followed by a little-
//!   endian `u16` encoding `distance - 1`;
//! - `0xc0`: an extended copy followed by a little-endian `u16` encoding
//!   `length - 67` and a little-endian `u16` encoding `distance - 1`.
//!
//! Copies must refer to the already-produced output and may overlap. All other
//! tag values are rejected. The encoder has a fixed 64 KiB backward window and
//! a deterministic 64-candidate match search.

use std::fmt;

const MIN_MATCH: usize = 3;
const SHORT_MATCH_MAX: usize = 66;
const EXTENDED_MATCH_BASE: usize = SHORT_MATCH_MAX + 1;
const MAX_MATCH: usize = EXTENDED_MATCH_BASE + 65_535;
const WINDOW: usize = 65_536;
const HASH_BITS: usize = 16;
const HASH_SIZE: usize = 1 << HASH_BITS;
const MAX_CHAIN: usize = 64;
const NO_POSITION: usize = usize::MAX;

/// A malformed `PFG-LZ/1` stream failed its bounded decoder validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// A tag requires bytes that are not present in the encoded stream.
    Truncated,
    /// A tag is not part of the `PFG-LZ/1` grammar.
    InvalidTag(u8),
    /// A literal or copy would exceed the caller-provided decoded length.
    OutputOverflow,
    /// The stream ended before producing the caller-provided decoded length.
    OutputUnderflow,
    /// A copy points before the beginning of the already-produced output.
    DistanceUnderflow,
    /// Bytes remain after the exact decoded length has been reached.
    TrailingData,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => formatter.write_str("PFG-LZ stream is truncated"),
            Self::InvalidTag(tag) => write!(formatter, "PFG-LZ stream has invalid tag {tag:#04x}"),
            Self::OutputOverflow => formatter.write_str("PFG-LZ stream exceeds decoded length"),
            Self::OutputUnderflow => {
                formatter.write_str("PFG-LZ stream ends before decoded length")
            }
            Self::DistanceUnderflow => {
                formatter.write_str("PFG-LZ copy distance underflows output")
            }
            Self::TrailingData => formatter.write_str("PFG-LZ stream has trailing data"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Deterministically encodes `input` with the `PFG-LZ/1` grammar.
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut heads = vec![NO_POSITION; HASH_SIZE];
    let mut previous = vec![NO_POSITION; input.len()];
    let mut position = 0;
    let mut literal_start = 0;

    while position < input.len() {
        let (match_length, distance) = find_match(input, position, &heads, &previous);
        if match_length < MIN_MATCH {
            insert_position(input, position, &mut heads, &mut previous);
            position += 1;
            continue;
        }

        emit_literals(&mut output, &input[literal_start..position]);
        emit_match(&mut output, match_length, distance);
        let match_end = position + match_length;
        while position < match_end {
            insert_position(input, position, &mut heads, &mut previous);
            position += 1;
        }
        literal_start = position;
    }
    emit_literals(&mut output, &input[literal_start..]);
    output
}

/// Decodes a complete `PFG-LZ/1` stream to its authenticated expected length.
///
/// # Errors
///
/// Returns [`DecodeError`] when the stream is malformed, non-canonical for the
/// supplied length, or would read outside its already-produced output.
pub fn decode(input: &[u8], expected_length: usize) -> Result<Vec<u8>, DecodeError> {
    let mut output = Vec::with_capacity(expected_length);
    let mut cursor = 0;

    while cursor < input.len() {
        if output.len() == expected_length {
            return Err(DecodeError::TrailingData);
        }
        let tag = input[cursor];
        cursor += 1;
        match tag {
            0x00..=0x7f => {
                let length = usize::from(tag) + 1;
                let end = cursor.checked_add(length).ok_or(DecodeError::Truncated)?;
                let literals = input.get(cursor..end).ok_or(DecodeError::Truncated)?;
                append_literals(&mut output, literals, expected_length)?;
                cursor = end;
            }
            0x80..=0xbf => {
                let length = MIN_MATCH + usize::from(tag & 0x3f);
                let distance = read_distance(input, &mut cursor)?;
                append_copy(&mut output, length, distance, expected_length)?;
            }
            0xc0 => {
                let extra_length = usize::from(read_u16(input, &mut cursor)?);
                let length = EXTENDED_MATCH_BASE + extra_length;
                let distance = read_distance(input, &mut cursor)?;
                append_copy(&mut output, length, distance, expected_length)?;
            }
            invalid => return Err(DecodeError::InvalidTag(invalid)),
        }
    }

    if output.len() != expected_length {
        return Err(DecodeError::OutputUnderflow);
    }
    Ok(output)
}

fn find_match(
    input: &[u8],
    position: usize,
    heads: &[usize],
    previous: &[usize],
) -> (usize, usize) {
    if input.len() - position < MIN_MATCH {
        return (0, 0);
    }
    let mut candidate = heads[hash_at(input, position)];
    let mut depth = 0;
    let mut best_length = 0;
    let mut best_distance = 0;
    let maximum_length = (input.len() - position).min(MAX_MATCH);

    while candidate != NO_POSITION && depth < MAX_CHAIN {
        let distance = position - candidate;
        if distance > WINDOW {
            break;
        }
        if input[candidate] == input[position]
            && input[candidate + 1] == input[position + 1]
            && input[candidate + 2] == input[position + 2]
        {
            let mut length = MIN_MATCH;
            while length < maximum_length && input[candidate + length] == input[position + length] {
                length += 1;
            }
            if length > best_length {
                best_length = length;
                best_distance = distance;
                if length == maximum_length {
                    break;
                }
            }
        }
        candidate = previous[candidate];
        depth += 1;
    }

    (best_length, best_distance)
}

fn insert_position(input: &[u8], position: usize, heads: &mut [usize], previous: &mut [usize]) {
    if input.len() - position < MIN_MATCH {
        return;
    }
    let slot = hash_at(input, position);
    previous[position] = heads[slot];
    heads[slot] = position;
}

fn hash_at(input: &[u8], position: usize) -> usize {
    let value = (usize::from(input[position]) << 16)
        ^ (usize::from(input[position + 1]) << 8)
        ^ usize::from(input[position + 2]);
    value.wrapping_mul(0x9e37) & (HASH_SIZE - 1)
}

fn emit_literals(output: &mut Vec<u8>, literals: &[u8]) {
    for chunk in literals.chunks(128) {
        let length = u8::try_from(chunk.len() - 1).expect("literal chunk is bounded");
        output.push(length);
        output.extend_from_slice(chunk);
    }
}

fn emit_match(output: &mut Vec<u8>, length: usize, distance: usize) {
    debug_assert!((MIN_MATCH..=MAX_MATCH).contains(&length));
    debug_assert!((1..=WINDOW).contains(&distance));
    let stored_distance = u16::try_from(distance - 1).expect("distance is bounded");
    if length <= SHORT_MATCH_MAX {
        let tag = 0x80 | u8::try_from(length - MIN_MATCH).expect("short match is bounded");
        output.push(tag);
    } else {
        output.push(0xc0);
        let stored_length =
            u16::try_from(length - EXTENDED_MATCH_BASE).expect("extended match is bounded");
        output.extend_from_slice(&stored_length.to_le_bytes());
    }
    output.extend_from_slice(&stored_distance.to_le_bytes());
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, DecodeError> {
    let end = cursor.checked_add(2).ok_or(DecodeError::Truncated)?;
    let bytes = input.get(*cursor..end).ok_or(DecodeError::Truncated)?;
    *cursor = end;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_distance(input: &[u8], cursor: &mut usize) -> Result<usize, DecodeError> {
    Ok(usize::from(read_u16(input, cursor)?) + 1)
}

fn append_literals(
    output: &mut Vec<u8>,
    literals: &[u8],
    expected_length: usize,
) -> Result<(), DecodeError> {
    let end = output
        .len()
        .checked_add(literals.len())
        .ok_or(DecodeError::OutputOverflow)?;
    if end > expected_length {
        return Err(DecodeError::OutputOverflow);
    }
    output.extend_from_slice(literals);
    Ok(())
}

fn append_copy(
    output: &mut Vec<u8>,
    length: usize,
    distance: usize,
    expected_length: usize,
) -> Result<(), DecodeError> {
    if distance > output.len() {
        return Err(DecodeError::DistanceUnderflow);
    }
    let end = output
        .len()
        .checked_add(length)
        .ok_or(DecodeError::OutputOverflow)?;
    if end > expected_length {
        return Err(DecodeError::OutputOverflow);
    }
    for _ in 0..length {
        let source = output.len() - distance;
        output.push(output[source]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DecodeError, decode, encode};

    #[test]
    fn deterministic_round_trip_covers_literal_and_copy_forms() {
        let mut input = Vec::new();
        for index in 0..2048 {
            input.push(u8::try_from(index % 251).unwrap());
        }
        input.extend(std::iter::repeat_n(0xa5, 70_000));
        let repeated_prefix = input[..4096].to_vec();
        input.extend_from_slice(&repeated_prefix);

        let first = encode(&input);
        let second = encode(&input);
        assert_eq!(first, second);
        assert_eq!(decode(&first, input.len()).unwrap(), input);
    }

    #[test]
    fn round_trips_boundaries_and_pseudorandom_data() {
        for length in [0, 1, 2, 3, 65, 66, 67, 127, 128, 129, 4095, 4096] {
            let mut state = 0x51a7_3c29_u32;
            let mut input = Vec::with_capacity(length);
            for _ in 0..length {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                input.push(u8::try_from(state >> 24).unwrap());
            }
            let encoded = encode(&input);
            assert_eq!(decode(&encoded, length).unwrap(), input, "length {length}");
        }
    }

    #[test]
    fn accepts_overlapping_copy() {
        let stream = [0x00, b'a', 0x80, 0x00, 0x00];
        assert_eq!(decode(&stream, 4).unwrap(), b"aaaa");
    }

    #[test]
    fn rejects_malformed_ranges_and_trailing_data() {
        assert_eq!(decode(&[0x00], 1), Err(DecodeError::Truncated));
        assert_eq!(
            decode(&[0x80, 0x00, 0x00], 3),
            Err(DecodeError::DistanceUnderflow)
        );
        assert_eq!(decode(&[0xff], 1), Err(DecodeError::InvalidTag(0xff)));
        assert_eq!(
            decode(&[0x00, 0x42, 0x00, 0x43], 1),
            Err(DecodeError::TrailingData)
        );
        assert_eq!(decode(&[0x00, 0x42], 0), Err(DecodeError::TrailingData));
    }
}
