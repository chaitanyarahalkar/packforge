use std::env;
use std::fs;

const MASK_TO_ALLOWED_STATUS: [bool; 8] = [true, true, true, false, true, false, false, false];
const MASK_TO_BIT_NUMBER: [usize; 8] = [0, 1, 2, 2, 3, 3, 3, 3];

fn test_most_significant_byte(byte: u8) -> bool {
    byte == 0 || byte == 0xff
}

// Reversible x86 BCJ transform based on the public-domain XZ Embedded
// simple/x86 filter algorithm.
fn x86_bcj(bytes: &mut [u8], encoding: bool) {
    if bytes.len() <= 4 {
        return;
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
                let byte = bytes[position + 4 - MASK_TO_BIT_NUMBER[previous_mask]];
                if !MASK_TO_ALLOWED_STATUS[previous_mask] || test_most_significant_byte(byte) {
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
        if test_most_significant_byte(bytes[position + 4]) {
            let mut source = u32::from_le_bytes(
                bytes[position + 1..position + 5]
                    .try_into()
                    .expect("fixed BCJ operand"),
            );
            let mut destination;
            loop {
                let program_counter = u32::try_from(position)
                    .expect("M2 fixture is below 4 GiB")
                    .wrapping_add(5);
                destination = if encoding {
                    source.wrapping_add(program_counter)
                } else {
                    source.wrapping_sub(program_counter)
                };
                if previous_mask == 0 {
                    break;
                }
                let shift = u32::try_from(MASK_TO_BIT_NUMBER[previous_mask] * 8)
                    .expect("BCJ shift fits u32");
                let byte = (destination >> (24 - shift)).to_le_bytes()[0];
                if !test_most_significant_byte(byte) {
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
}

fn main() {
    let input_path = env::args().nth(1).expect("usage: m2_codec_spike <ELF>");
    let original = fs::read(input_path).expect("cannot read input");
    assert!(!original.is_empty(), "input must not be empty");

    let mut transformed = original.clone();
    x86_bcj(&mut transformed, true);

    println!("streams\ttransformed_bytes\tpayload_bytes\tmax_output_chunk_bytes");
    for streams in 1..=4usize {
        let mut encoded_chunks = Vec::with_capacity(streams);
        let mut reconstructed = Vec::with_capacity(original.len());
        let mut maximum_output = 0usize;
        for index in 0..streams {
            let start = transformed.len() * index / streams;
            let end = transformed.len() * (index + 1) / streams;
            let chunk = &transformed[start..end];
            maximum_output = maximum_output.max(chunk.len());
            let properties = lzma_sdk_rs::LzmaProps::for_level(
                9,
                u32::try_from(chunk.len()).expect("chunk length fits u32"),
            );
            let encoded = lzma_sdk_rs::encode(chunk, &properties);
            let decoder_properties = lzma_sdk_rs::decoder_props(&properties);
            let mut decoded = vec![0u8; chunk.len()];
            packforge_lzma_decoder::decompress(&encoded, &decoder_properties, &mut decoded)
                .expect("shared decoder rejected spike stream");
            assert_eq!(decoded, chunk, "chunk decode mismatch");
            reconstructed.extend_from_slice(&decoded);
            encoded_chunks.push(encoded);
        }

        x86_bcj(&mut reconstructed, false);
        assert_eq!(reconstructed, original, "BCJ reconstruction mismatch");
        let payload_bytes = encoded_chunks.iter().map(Vec::len).sum::<usize>();
        println!(
            "{streams}\t{}\t{payload_bytes}\t{maximum_output}",
            transformed.len()
        );
    }
}
