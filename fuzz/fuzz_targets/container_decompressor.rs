#![no_main]

use libfuzzer_sys::fuzz_target;
use packforge_core::{PackOptions, Profile, pack, verify};

const HEADER_LEN: usize = 192;
const HEADER_HASH_OFFSET: usize = 152;
const HEADER_HASH_END: usize = 184;

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0u8; 16_384];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x40_1000u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[96..104].copy_from_slice(&16_384u64.to_le_bytes());
    bytes[104..112].copy_from_slice(&16_384u64.to_le_bytes());
    bytes[256..].fill(0x41);
    bytes
}

fuzz_target!(|input: &[u8]| {
    let Some((&selector, encoded)) = input.split_first() else {
        return;
    };
    if encoded.is_empty() {
        return;
    }
    let profile = if selector & 1 == 0 {
        Profile::Fast
    } else {
        Profile::Balanced
    };
    let mut container = pack(
        &fixture(),
        0o755,
        PackOptions {
            profile,
            allow_larger: true,
        },
    )
    .unwrap()
    .bytes;

    let declared_size = input
        .get(1..5)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .map_or(16_384, |size| u64::from(size % 1_048_576) + 1);
    container[32..40].copy_from_slice(&declared_size.to_le_bytes());
    container[40..48].copy_from_slice(&u64::try_from(encoded.len()).unwrap().to_le_bytes());
    container[120..152].copy_from_slice(blake3::hash(encoded).as_bytes());
    container[HEADER_HASH_OFFSET..HEADER_HASH_END].fill(0);
    container.truncate(HEADER_LEN);
    container.extend_from_slice(encoded);
    let header_hash = *blake3::hash(&container[..HEADER_LEN]).as_bytes();
    container[HEADER_HASH_OFFSET..HEADER_HASH_END].copy_from_slice(&header_hash);

    let _ = verify(&container);
});
