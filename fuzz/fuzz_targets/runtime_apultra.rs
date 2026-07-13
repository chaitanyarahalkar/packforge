#![no_main]

use libfuzzer_sys::fuzz_target;
use packforge_runtime_linux_x86_64::apultra;

const MAX_FUZZ_OUTPUT: usize = 1 << 20;

fuzz_target!(|input: &[u8]| {
    let Some(length) = input.get(..4) else {
        return;
    };
    let requested = u32::from_le_bytes(length.try_into().expect("fixed length")) as usize;
    let mut output = vec![0u8; requested % (MAX_FUZZ_OUTPUT + 1)];
    let _ = apultra::decompress(&input[4..], &mut output);
});
