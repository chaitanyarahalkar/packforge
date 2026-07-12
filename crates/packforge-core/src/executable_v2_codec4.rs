//! Deterministic four-stream LZMA1 plus x86 BCJ payload framing for M2.

use std::collections::{BTreeMap, btree_map::Entry};

use packforge_lzma_decoder as lzma;

use super::executable_v2::ExecutableV2Error;

pub(super) const CODEC_LZMA1_BCJ4: u16 = 4;
const CHUNK_COUNT: usize = 4;
const CHUNK_ENTRY_LEN: usize = 32;
pub(super) const CHUNK_TABLE_LEN: usize = CHUNK_COUNT * CHUNK_ENTRY_LEN;
const TRAILING_SHIFT: u32 = 56;
const LENGTH_MASK: u64 = (1u64 << TRAILING_SHIFT) - 1;
const MAX_CHUNK_PERCENT: usize = 28;

#[derive(Debug, Clone, Copy)]
struct ChunkEntry {
    decoded_offset: usize,
    decoded_length: usize,
    compressed_offset: usize,
    compressed_length: usize,
    trailing_bytes: u8,
}

pub(super) fn encode(
    original: &[u8],
    properties: &lzma_sdk_rs::LzmaProps,
) -> Result<Vec<u8>, ExecutableV2Error> {
    let mut transformed = original.to_vec();
    x86_bcj(&mut transformed, true)?;
    let maximum_chunk = transformed
        .len()
        .checked_mul(MAX_CHUNK_PERCENT)
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    let mut cache = BTreeMap::<(usize, usize), (Vec<u8>, u8)>::new();
    let coarse = candidate_boundaries(transformed.len());
    let best = evaluate_boundaries(
        &coarse,
        &mut cache,
        &transformed,
        properties,
        maximum_chunk,
        None,
    )?;
    let (_, _, selected) = best.ok_or(ExecutableV2Error::InvalidRange)?;
    let split = [0, selected[0], selected[1], selected[2], transformed.len()];
    let mut payload = vec![0u8; CHUNK_TABLE_LEN];
    let mut compressed_offset = CHUNK_TABLE_LEN;
    for (index, range) in split.windows(2).enumerate() {
        let (encoded, trailing_bytes) =
            encoded_chunk(&mut cache, &transformed, range[0], range[1], properties)?;
        let entry = ChunkEntry {
            decoded_offset: range[0],
            decoded_length: range[1] - range[0],
            compressed_offset,
            compressed_length: encoded.len(),
            trailing_bytes: *trailing_bytes,
        };
        encode_entry(
            &mut payload[index * CHUNK_ENTRY_LEN..][..CHUNK_ENTRY_LEN],
            entry,
        )?;
        payload.extend_from_slice(encoded);
        compressed_offset = compressed_offset
            .checked_add(encoded.len())
            .ok_or(ExecutableV2Error::InvalidRange)?;
    }
    Ok(payload)
}

pub(super) fn decode(
    payload: &[u8],
    properties: [u8; 5],
    original_length: usize,
) -> Result<Vec<u8>, ExecutableV2Error> {
    let entries = parse_entries(payload, original_length)?;
    let mut transformed = vec![0u8; original_length];
    for entry in entries {
        let compressed_end = entry
            .compressed_offset
            .checked_add(entry.compressed_length)
            .ok_or(ExecutableV2Error::InvalidRange)?;
        let decoded_end = entry
            .decoded_offset
            .checked_add(entry.decoded_length)
            .ok_or(ExecutableV2Error::InvalidRange)?;
        let report = lzma::decompress(
            payload
                .get(entry.compressed_offset..compressed_end)
                .ok_or(ExecutableV2Error::InvalidRange)?,
            &properties,
            transformed
                .get_mut(entry.decoded_offset..decoded_end)
                .ok_or(ExecutableV2Error::InvalidRange)?,
        )
        .map_err(ExecutableV2Error::Decompression)?;
        if report.trailing_bytes != entry.trailing_bytes {
            return Err(ExecutableV2Error::TrailingBytes {
                expected: entry.trailing_bytes,
                actual: report.trailing_bytes,
            });
        }
    }
    x86_bcj(&mut transformed, false)?;
    Ok(transformed)
}

fn encoded_chunk<'a>(
    cache: &'a mut BTreeMap<(usize, usize), (Vec<u8>, u8)>,
    transformed: &[u8],
    start: usize,
    end: usize,
    properties: &lzma_sdk_rs::LzmaProps,
) -> Result<&'a (Vec<u8>, u8), ExecutableV2Error> {
    match cache.entry((start, end)) {
        Entry::Occupied(entry) => Ok(entry.into_mut()),
        Entry::Vacant(entry) => {
            let encoded = lzma_sdk_rs::encode(&transformed[start..end], properties);
            let decoder_properties = lzma_sdk_rs::decoder_props(properties);
            let mut decoded = vec![0u8; end - start];
            let report = lzma::decompress(&encoded, &decoder_properties, &mut decoded)
                .map_err(ExecutableV2Error::Decompression)?;
            if decoded != transformed[start..end] {
                return Err(ExecutableV2Error::Integrity("codec-4 chunk"));
            }
            Ok(entry.insert((encoded, report.trailing_bytes)))
        }
    }
}

fn candidate_boundaries(length: usize) -> [Vec<usize>; 3] {
    core::array::from_fn(|index| {
        let center = (index + 1) * 25;
        ((center - 3)..=(center + 3))
            .map(|percent| length * percent / 100)
            .collect()
    })
}

fn evaluate_boundaries(
    boundaries: &[Vec<usize>; 3],
    cache: &mut BTreeMap<(usize, usize), (Vec<u8>, u8)>,
    transformed: &[u8],
    properties: &lzma_sdk_rs::LzmaProps,
    maximum_chunk: usize,
    mut best: Option<(usize, usize, [usize; 3])>,
) -> Result<Option<(usize, usize, [usize; 3])>, ExecutableV2Error> {
    for &first in &boundaries[0] {
        for &second in &boundaries[1] {
            for &third in &boundaries[2] {
                let split = [0, first, second, third, transformed.len()];
                if split
                    .windows(2)
                    .any(|range| range[0] >= range[1] || range[1] - range[0] > maximum_chunk)
                {
                    continue;
                }
                let mut total = CHUNK_TABLE_LEN;
                let mut maximum_encoded = 0usize;
                for range in split.windows(2) {
                    let (encoded, _) =
                        encoded_chunk(cache, transformed, range[0], range[1], properties)?;
                    total = total
                        .checked_add(encoded.len())
                        .ok_or(ExecutableV2Error::InvalidRange)?;
                    maximum_encoded = maximum_encoded.max(encoded.len());
                }
                let score = (total, maximum_encoded, [first, second, third]);
                if best.as_ref().is_none_or(|current| score < *current) {
                    best = Some(score);
                }
            }
        }
    }
    Ok(best)
}

fn encode_entry(output: &mut [u8], entry: ChunkEntry) -> Result<(), ExecutableV2Error> {
    if entry.decoded_length == 0
        || entry.decoded_length as u64 > LENGTH_MASK
        || entry.trailing_bytes > 5
    {
        return Err(ExecutableV2Error::InvalidRange);
    }
    put_u64(output, 0, to_u64(entry.decoded_offset)?);
    put_u64(
        output,
        8,
        to_u64(entry.decoded_length)? | (u64::from(entry.trailing_bytes) << TRAILING_SHIFT),
    );
    put_u64(output, 16, to_u64(entry.compressed_offset)?);
    put_u64(output, 24, to_u64(entry.compressed_length)?);
    Ok(())
}

fn parse_entries(
    payload: &[u8],
    original_length: usize,
) -> Result<[ChunkEntry; CHUNK_COUNT], ExecutableV2Error> {
    if payload.len() <= CHUNK_TABLE_LEN {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let maximum_chunk = original_length
        .checked_mul(MAX_CHUNK_PERCENT)
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    let mut expected_decoded = 0usize;
    let mut expected_compressed = CHUNK_TABLE_LEN;
    let mut entries = [ChunkEntry {
        decoded_offset: 0,
        decoded_length: 0,
        compressed_offset: 0,
        compressed_length: 0,
        trailing_bytes: 0,
    }; CHUNK_COUNT];
    for (index, entry) in entries.iter_mut().enumerate() {
        let bytes = &payload[index * CHUNK_ENTRY_LEN..][..CHUNK_ENTRY_LEN];
        let encoded_length = get_u64(bytes, 8);
        let decoded_length = to_usize(encoded_length & LENGTH_MASK)?;
        *entry = ChunkEntry {
            decoded_offset: to_usize(get_u64(bytes, 0))?,
            decoded_length,
            compressed_offset: to_usize(get_u64(bytes, 16))?,
            compressed_length: to_usize(get_u64(bytes, 24))?,
            trailing_bytes: (encoded_length >> TRAILING_SHIFT) as u8,
        };
        if entry.decoded_offset != expected_decoded
            || entry.compressed_offset != expected_compressed
            || entry.decoded_length == 0
            || entry.decoded_length > maximum_chunk
            || entry.compressed_length == 0
            || entry.trailing_bytes > 5
        {
            return Err(ExecutableV2Error::InvalidRange);
        }
        expected_decoded = expected_decoded
            .checked_add(entry.decoded_length)
            .ok_or(ExecutableV2Error::InvalidRange)?;
        expected_compressed = expected_compressed
            .checked_add(entry.compressed_length)
            .ok_or(ExecutableV2Error::InvalidRange)?;
    }
    if expected_decoded != original_length || expected_compressed != payload.len() {
        return Err(ExecutableV2Error::InvalidRange);
    }
    Ok(entries)
}

fn x86_bcj(bytes: &mut [u8], encoding: bool) -> Result<(), ExecutableV2Error> {
    const ALLOWED: [bool; 8] = [true, true, true, false, true, false, false, false];
    const BIT_NUMBER: [usize; 8] = [0, 1, 2, 2, 3, 3, 3, 3];
    if bytes.len() <= 4 {
        return Ok(());
    }
    let limit = bytes.len() - 4;
    let mut position = 0usize;
    let mut previous_position = usize::MAX;
    let mut previous_mask = 0usize;
    while position < limit {
        if bytes[position] & 0xfe != 0xe8 {
            position += 1;
            continue;
        }
        previous_position = position.wrapping_sub(previous_position);
        if previous_position <= 3 {
            previous_mask = (previous_mask << (previous_position - 1)) & 7;
            if previous_mask != 0 {
                let byte = bytes[position + 4 - BIT_NUMBER[previous_mask]];
                if !ALLOWED[previous_mask] || byte == 0 || byte == 0xff {
                    previous_position = position;
                    previous_mask = (previous_mask << 1) | 1;
                    position += 1;
                    continue;
                }
            }
        } else {
            previous_mask = 0;
        }
        previous_position = position;
        if matches!(bytes[position + 4], 0 | 0xff) {
            let mut source = u32::from_le_bytes(
                bytes[position + 1..position + 5]
                    .try_into()
                    .map_err(|_| ExecutableV2Error::InvalidRange)?,
            );
            let mut destination;
            loop {
                let program_counter = u32::try_from(position)
                    .map_err(|_| ExecutableV2Error::InvalidRange)?
                    .wrapping_add(5);
                destination = if encoding {
                    source.wrapping_add(program_counter)
                } else {
                    source.wrapping_sub(program_counter)
                };
                if previous_mask == 0 {
                    break;
                }
                let shift = u32::try_from(BIT_NUMBER[previous_mask] * 8)
                    .map_err(|_| ExecutableV2Error::InvalidRange)?;
                let byte = (destination >> (24 - shift)).to_le_bytes()[0];
                if !matches!(byte, 0 | 0xff) {
                    break;
                }
                source = destination ^ ((1u32 << (32 - shift)) - 1);
            }
            destination &= 0x01ff_ffff;
            destination |= 0u32.wrapping_sub(destination & 0x0100_0000);
            bytes[position + 1..position + 5].copy_from_slice(&destination.to_le_bytes());
            position += 5;
        } else {
            previous_mask = (previous_mask << 1) | 1;
            position += 1;
        }
    }
    Ok(())
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        input[offset..offset + 8]
            .try_into()
            .expect("fixed codec-4 entry"),
    )
}

fn to_u64(value: usize) -> Result<u64, ExecutableV2Error> {
    u64::try_from(value).map_err(|_| ExecutableV2Error::InvalidRange)
}

fn to_usize(value: u64) -> Result<usize, ExecutableV2Error> {
    usize::try_from(value).map_err(|_| ExecutableV2Error::InvalidRange)
}
