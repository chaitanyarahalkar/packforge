#![no_main]

use libfuzzer_sys::fuzz_target;
use packforge_runtime_linux_x86_64::v2_format::{
    self, HEADER_LEN, MANIFEST_HEADER_LEN, TRAILER_LEN,
};

fuzz_target!(|input: &[u8]| {
    if let Some(bytes) = input.get(..TRAILER_LEN) {
        let trailer_bytes: &[u8; TRAILER_LEN] = bytes.try_into().expect("fixed trailer length");
        let _ = v2_format::parse_trailer(trailer_bytes, input.len() as u64);
    }

    if let Some(bytes) = input.get(..HEADER_LEN) {
        let header_bytes: &[u8; HEADER_LEN] = bytes.try_into().expect("fixed header length");
        let _ = v2_format::parse_header(header_bytes);
    }

    if input.len() >= MANIFEST_HEADER_LEN {
        if let Ok(manifest) = v2_format::parse_manifest(input, input.len() as u64) {
            let _ = v2_format::validate_elf(input, &manifest);
        }
    }
});
