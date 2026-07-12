#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(not(feature = "optimized-hash"))]
pub mod hash;
#[cfg(feature = "optimized-hash")]
pub use packforge_runtime_hash as hash;
pub mod lz4;
#[cfg(feature = "lzma")]
pub mod lzma;
pub mod procfd;
#[cfg(feature = "lzma")]
pub mod v2_format;
