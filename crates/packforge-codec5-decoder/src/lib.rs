//! Allocation-free runtime decoders for executable-v2 codec 5.

#![no_std]

#[cfg(test)]
extern crate std;

/// Checked `APultra` byte-stream decoding.
pub mod apultra {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../runtime/linux-x86_64/src/apultra.rs"
    ));
}

/// Checked BCJ2 merge decoding.
#[allow(clippy::cast_possible_truncation, clippy::incompatible_msrv)]
pub mod bcj2 {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../runtime/linux-x86_64/src/bcj2.rs"
    ));
}
