#![no_main]

use libfuzzer_sys::fuzz_target;
use packforge_runtime_linux_x86_64::hash;

fuzz_target!(|input: &[u8]| {
    assert_eq!(hash::hash(input), *blake3::hash(input).as_bytes());
});
