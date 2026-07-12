//! Codec 5 `APultra` plus BCJ2 host framing.

use packforge_codec5_sys::{
    apultra_compress_bytes, apultra_decompress_bytes, bcj2_decode, bcj2_encode,
};

use super::executable_v2::{ExecutableV2Error, digest};

pub(super) const CODEC_APULTRA_BCJ2: u16 = 5;
pub(super) const TABLE_LEN: usize = 160;
const TABLE_LEN_U16: u16 = 160;
const STREAM_COUNT: usize = 4;
const TABLE_MAGIC: &[u8; 8] = b"PFGBCJ05";

#[derive(Debug, Clone, Copy)]
struct Entry {
    decoded_length: usize,
    compressed_offset: usize,
    compressed_length: usize,
}

pub(super) struct Encoded {
    pub payload: Vec<u8>,
}

pub(super) fn encode(
    original: &[u8],
    runtime_length: usize,
    properties: &lzma_sdk_rs::LzmaProps,
) -> Result<Encoded, ExecutableV2Error> {
    let runtime = original
        .get(..runtime_length)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    if runtime.is_empty() {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let streams = bcj2_encode(runtime).map_err(|_| ExecutableV2Error::Integrity("bcj2"))?;
    let jump = transpose(&streams[2])?;
    let encoded = [
        apultra_compress_bytes(&streams[0])
            .map_err(|_| ExecutableV2Error::Integrity("apultra main"))?,
        compress_ap_or_empty(&streams[1], "apultra call")?,
        compress_ap_or_empty(&jump, "apultra jump")?,
        streams[3].clone(),
    ];
    let mut payload = vec![0u8; TABLE_LEN];
    payload[..8].copy_from_slice(TABLE_MAGIC);
    put_u16(&mut payload, 8, 1);
    put_u16(&mut payload, 10, TABLE_LEN_U16);
    put_u64(&mut payload, 16, to_u64(runtime_length)?);
    payload[24..56].copy_from_slice(&digest(runtime));
    let mut compressed_offset = TABLE_LEN;
    for index in 0..STREAM_COUNT {
        let entry = Entry {
            decoded_length: streams[index].len(),
            compressed_offset,
            compressed_length: encoded[index].len(),
        };
        encode_entry(&mut payload[64 + index * 24..][..24], entry)?;
        payload.extend_from_slice(&encoded[index]);
        compressed_offset = compressed_offset
            .checked_add(encoded[index].len())
            .ok_or(ExecutableV2Error::InvalidRange)?;
    }
    let tail = &original[runtime_length..];
    let tail_trailing_bytes = if tail.is_empty() {
        0
    } else {
        let encoded_tail = lzma_sdk_rs::encode(tail, properties);
        let mut decoded = vec![0u8; tail.len()];
        let report = packforge_lzma_decoder::decompress(
            &encoded_tail,
            &lzma_sdk_rs::decoder_props(properties),
            &mut decoded,
        )
        .map_err(ExecutableV2Error::Decompression)?;
        if decoded != tail {
            return Err(ExecutableV2Error::Integrity("recovery tail"));
        }
        payload.extend_from_slice(&encoded_tail);
        report.trailing_bytes
    };
    payload[56] = tail_trailing_bytes;
    let decoded = decode(
        &payload,
        lzma_sdk_rs::decoder_props(properties),
        original.len(),
    )?;
    if decoded != original {
        return Err(ExecutableV2Error::Integrity("codec 5"));
    }
    Ok(Encoded { payload })
}

pub(super) fn decode(
    payload: &[u8],
    properties: [u8; 5],
    original_length: usize,
) -> Result<Vec<u8>, ExecutableV2Error> {
    let (runtime_length, runtime_digest, tail_trailing, entries) = parse(payload, original_length)?;
    let mut decoded_streams = Vec::with_capacity(STREAM_COUNT);
    for (index, entry) in entries.iter().copied().enumerate() {
        let end = entry
            .compressed_offset
            .checked_add(entry.compressed_length)
            .ok_or(ExecutableV2Error::InvalidRange)?;
        let compressed = &payload[entry.compressed_offset..end];
        let decoded = if index == 3 {
            compressed.to_vec()
        } else if entry.decoded_length == 0 {
            Vec::new()
        } else {
            apultra_decompress_bytes(compressed, entry.decoded_length)
                .map_err(|_| ExecutableV2Error::Integrity("apultra stream"))?
        };
        decoded_streams.push(decoded);
    }
    decoded_streams[2] = untranspose(&decoded_streams[2])?;
    let runtime = bcj2_decode(
        [
            &decoded_streams[0],
            &decoded_streams[1],
            &decoded_streams[2],
            &decoded_streams[3],
        ],
        runtime_length,
    )
    .map_err(|_| ExecutableV2Error::Integrity("bcj2"))?;
    if digest(&runtime) != runtime_digest {
        return Err(ExecutableV2Error::Integrity("runtime image"));
    }
    let mut original = runtime;
    let tail_length = original_length - runtime_length;
    let tail_offset = entries[3]
        .compressed_offset
        .checked_add(entries[3].compressed_length)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    if tail_length == 0 {
        if tail_offset != payload.len() || tail_trailing != 0 {
            return Err(ExecutableV2Error::InvalidRange);
        }
    } else {
        let mut tail = vec![0u8; tail_length];
        let report =
            packforge_lzma_decoder::decompress(&payload[tail_offset..], &properties, &mut tail)
                .map_err(ExecutableV2Error::Decompression)?;
        if report.trailing_bytes != tail_trailing {
            return Err(ExecutableV2Error::TrailingBytes {
                expected: tail_trailing,
                actual: report.trailing_bytes,
            });
        }
        original.extend_from_slice(&tail);
    }
    Ok(original)
}

fn parse(
    payload: &[u8],
    original_length: usize,
) -> Result<(usize, [u8; 32], u8, [Entry; STREAM_COUNT]), ExecutableV2Error> {
    if payload.len() <= TABLE_LEN
        || payload.get(..8) != Some(TABLE_MAGIC)
        || get_u16(payload, 8) != 1
        || usize::from(get_u16(payload, 10)) != TABLE_LEN
        || payload[12..16].iter().any(|byte| *byte != 0)
        || payload[57..64].iter().any(|byte| *byte != 0)
    {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let runtime_length = to_usize(get_u64(payload, 16))?;
    if runtime_length == 0 || runtime_length > original_length {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let runtime_digest = payload[24..56]
        .try_into()
        .map_err(|_| ExecutableV2Error::InvalidRange)?;
    let mut expected_offset = TABLE_LEN;
    let mut entries = [Entry {
        decoded_length: 0,
        compressed_offset: 0,
        compressed_length: 0,
    }; STREAM_COUNT];
    for (index, entry) in entries.iter_mut().enumerate() {
        let input = &payload[64 + index * 24..][..24];
        *entry = Entry {
            decoded_length: to_usize(get_u64(input, 0))?,
            compressed_offset: to_usize(get_u64(input, 8))?,
            compressed_length: to_usize(get_u64(input, 16))?,
        };
        if (matches!(index, 0 | 3) && (entry.decoded_length == 0 || entry.compressed_length == 0))
            || entry.compressed_offset != expected_offset
            || entry.compressed_offset > payload.len()
            || entry.compressed_length > payload.len() - entry.compressed_offset
            || (matches!(index, 1 | 2) && entry.decoded_length % 4 != 0)
            || (index == 3 && entry.decoded_length != entry.compressed_length)
            || (index < 3 && (entry.decoded_length == 0) != (entry.compressed_length == 0))
        {
            return Err(ExecutableV2Error::InvalidRange);
        }
        expected_offset = expected_offset
            .checked_add(entry.compressed_length)
            .ok_or(ExecutableV2Error::InvalidRange)?;
    }
    if entries[0]
        .decoded_length
        .checked_add(entries[1].decoded_length)
        .and_then(|length| length.checked_add(entries[2].decoded_length))
        != Some(runtime_length)
        || expected_offset > payload.len()
    {
        return Err(ExecutableV2Error::InvalidRange);
    }
    Ok((runtime_length, runtime_digest, payload[56], entries))
}

fn compress_ap_or_empty(input: &[u8], range: &'static str) -> Result<Vec<u8>, ExecutableV2Error> {
    if input.is_empty() {
        Ok(Vec::new())
    } else {
        apultra_compress_bytes(input).map_err(|_| ExecutableV2Error::Integrity(range))
    }
}

fn transpose(input: &[u8]) -> Result<Vec<u8>, ExecutableV2Error> {
    if input.len() % 4 != 0 {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let values = input.len() / 4;
    let mut output = vec![0u8; input.len()];
    for index in 0..values {
        for byte in 0..4 {
            output[byte * values + index] = input[index * 4 + byte];
        }
    }
    Ok(output)
}

fn untranspose(input: &[u8]) -> Result<Vec<u8>, ExecutableV2Error> {
    if input.len() % 4 != 0 {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let values = input.len() / 4;
    let mut output = vec![0u8; input.len()];
    for index in 0..values {
        for byte in 0..4 {
            output[index * 4 + byte] = input[byte * values + index];
        }
    }
    Ok(output)
}

fn encode_entry(output: &mut [u8], entry: Entry) -> Result<(), ExecutableV2Error> {
    put_u64(output, 0, to_u64(entry.decoded_length)?);
    put_u64(output, 8, to_u64(entry.compressed_offset)?);
    put_u64(output, 16, to_u64(entry.compressed_length)?);
    Ok(())
}

fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([input[offset], input[offset + 1]])
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(input[offset..offset + 8].try_into().expect("fixed u64"))
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn to_u64(value: usize) -> Result<u64, ExecutableV2Error> {
    u64::try_from(value).map_err(|_| ExecutableV2Error::InvalidRange)
}

fn to_usize(value: u64) -> Result<usize, ExecutableV2Error> {
    usize::try_from(value).map_err(|_| ExecutableV2Error::InvalidRange)
}

#[cfg(test)]
mod tests {
    use super::{ExecutableV2Error, TABLE_LEN, decode, encode, put_u64};

    fn fixture() -> Vec<u8> {
        let mut bytes: Vec<u8> = (0u8..=u8::MAX)
            .cycle()
            .take(8192)
            .map(|value| value.wrapping_mul(17).wrapping_add(3))
            .collect();
        for offset in (128..4096).step_by(32) {
            bytes[offset] = 0xe8;
            bytes[offset + 1..offset + 5]
                .copy_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());
        }
        bytes
    }

    fn properties(length: usize) -> lzma_sdk_rs::LzmaProps {
        lzma_sdk_rs::LzmaProps::for_level(9, u32::try_from(length).unwrap())
    }

    #[test]
    fn deterministic_round_trip_includes_recovery_tail() {
        let original = fixture();
        let properties = properties(original.len());
        let first = encode(&original, 4096, &properties).unwrap();
        let second = encode(&original, 4096, &properties).unwrap();
        assert_eq!(first.payload, second.payload);
        assert_eq!(
            decode(
                &first.payload,
                lzma_sdk_rs::decoder_props(&properties),
                original.len()
            )
            .unwrap(),
            original
        );
    }

    #[test]
    fn malformed_table_and_apultra_stream_fail_closed() {
        let original = fixture();
        let properties = properties(original.len());
        let encoded = encode(&original, 4096, &properties).unwrap();

        let mut invalid_offset = encoded.payload.clone();
        put_u64(&mut invalid_offset, 72, u64::MAX);
        assert!(matches!(
            decode(
                &invalid_offset,
                lzma_sdk_rs::decoder_props(&properties),
                original.len()
            ),
            Err(ExecutableV2Error::InvalidRange)
        ));

        let mut corrupt_stream = encoded.payload;
        corrupt_stream[TABLE_LEN] ^= 0x80;
        assert!(
            decode(
                &corrupt_stream,
                lzma_sdk_rs::decoder_props(&properties),
                original.len()
            )
            .is_err()
        );
    }
}
