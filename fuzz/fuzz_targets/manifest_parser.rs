#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok(manifest) = packforge_core::decode_manifest_v0(input) {
        let encoded = manifest.encode().expect("decoded manifest must re-encode");
        assert_eq!(encoded, input);
    }
});
