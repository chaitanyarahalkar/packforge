#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(not(feature = "optimized-hash"))]
pub mod hash;
#[cfg(feature = "optimized-hash")]
pub use packforge_runtime_hash as hash;
#[cfg(feature = "lzma-asm")]
pub mod bcj;
#[cfg(feature = "apultra-bcj2")]
pub use packforge_codec5_decoder::{apultra, bcj2};
pub mod lz4;
#[cfg(feature = "lzma")]
pub mod lzma;
#[cfg(feature = "lzma-asm")]
pub mod lzma_asm;
#[cfg(feature = "lzma-parallel")]
pub mod lzma_parallel;
pub mod procfd;
#[cfg(any(feature = "runtime-v2", feature = "lzma", feature = "lzma-asm"))]
pub mod v2_format;
