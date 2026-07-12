#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(not(feature = "optimized-hash"))]
pub mod hash;
#[cfg(feature = "optimized-hash")]
pub use packforge_runtime_hash as hash;
#[cfg(feature = "lzma-asm")]
pub mod bcj;
pub mod lz4;
#[cfg(feature = "lzma")]
pub mod lzma;
#[cfg(feature = "lzma-asm")]
pub mod lzma_asm;
pub mod procfd;
#[cfg(any(feature = "lzma", feature = "lzma-asm"))]
pub mod v2_format;
