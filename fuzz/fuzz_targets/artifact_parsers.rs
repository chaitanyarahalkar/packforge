#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let _ = packforge_core::inspect(input);
    let _ = packforge_core::verify(input);
    let _ = packforge_core::inspect_executable(input);
    let _ = packforge_core::verify_executable(input);
});
