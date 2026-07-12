#![no_std]

#[cfg(test)]
extern crate std;

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
mod compact {
    include!("../../../runtime/linux-x86_64/src/hash.rs");
}

pub use compact::hash;
