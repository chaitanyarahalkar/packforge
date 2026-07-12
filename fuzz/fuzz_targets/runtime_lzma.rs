#![no_main]

use libfuzzer_sys::fuzz_target;
use packforge_runtime_linux_x86_64::lzma;

const MAX_FUZZ_OUTPUT: usize = 1 << 20;

fuzz_target!(|input: &[u8]| {
    let Some(length_bytes) = input.get(..4) else {
        return;
    };
    let Some(properties) = input.get(4..9) else {
        return;
    };
    let requested = u32::from_le_bytes([
        length_bytes[0],
        length_bytes[1],
        length_bytes[2],
        length_bytes[3],
    ]) as usize;
    let output_length = requested % (MAX_FUZZ_OUTPUT + 1);
    let properties = [
        properties[0],
        properties[1],
        properties[2],
        properties[3],
        properties[4],
    ];
    let mut output = vec![0u8; output_length];
    let _ = lzma::decompress(&input[9..], &properties, &mut output);
});
