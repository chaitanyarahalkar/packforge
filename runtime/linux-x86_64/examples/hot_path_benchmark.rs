use std::env;
use std::fs;
use std::hint::black_box;
use std::ptr;
use std::time::Instant;

use packforge_runtime_linux_x86_64::{hash, lzma};

fn median(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn main() {
    let mut arguments = env::args().skip(1);
    let input_path = arguments
        .next()
        .expect("usage: hot_path_benchmark <input> [iterations] [level]");
    let iterations = arguments
        .next()
        .map_or(Ok(21usize), |value| value.parse::<usize>())
        .expect("iterations must be a positive integer");
    let level = arguments
        .next()
        .map_or(Ok(9u32), |value| value.parse::<u32>())
        .expect("level must be an integer from 0 through 9");
    assert!(iterations > 0, "iterations must be positive");
    assert!(level <= 9, "level must be from 0 through 9");
    assert!(arguments.next().is_none(), "too many arguments");

    let original = fs::read(&input_path).expect("cannot read input");
    assert!(!original.is_empty(), "input must not be empty");
    let loader =
        fs::read("../artifacts/linux-x86_64/loader-v2").expect("cannot read checked-in loader-v2");
    let properties = lzma_sdk_rs::LzmaProps::for_level(
        level,
        u32::try_from(original.len()).expect("input is too large"),
    );
    let payload = lzma_sdk_rs::encode(&original, &properties);
    let decoder_properties = lzma_sdk_rs::decoder_props(&properties);
    let expected_original_hash = hash::hash(&original);
    let mut output = vec![0u8; original.len()];
    let mut copied = vec![0u8; original.len()];

    let mut loader_hash_samples = Vec::with_capacity(iterations);
    let mut payload_hash_samples = Vec::with_capacity(iterations);
    let mut decode_samples = Vec::with_capacity(iterations);
    let mut original_hash_samples = Vec::with_capacity(iterations);
    let mut reference_hash_samples = Vec::with_capacity(iterations);
    let mut volatile_copy_samples = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        black_box(hash::hash(black_box(&loader)));
        loader_hash_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(hash::hash(black_box(&payload)));
        payload_hash_samples.push(start.elapsed().as_nanos());

        output.fill(0);
        let start = Instant::now();
        black_box(
            lzma::decompress(
                black_box(&payload),
                black_box(&decoder_properties),
                black_box(&mut output),
            )
            .expect("decode failed"),
        );
        decode_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        let original_hash = hash::hash(black_box(&output));
        original_hash_samples.push(start.elapsed().as_nanos());
        assert_eq!(
            original_hash, expected_original_hash,
            "decoded hash mismatch"
        );

        let start = Instant::now();
        black_box(blake3::hash(black_box(&output)));
        reference_hash_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        for (index, byte) in output.iter().copied().enumerate() {
            unsafe { ptr::write_volatile(copied.as_mut_ptr().add(index), byte) };
        }
        volatile_copy_samples.push(start.elapsed().as_nanos());
        assert_eq!(copied, original, "copy mismatch");
    }

    println!("phase\tmedian_ns");
    println!("loader_hash\t{}", median(&mut loader_hash_samples));
    println!("payload_hash\t{}", median(&mut payload_hash_samples));
    println!("decompress\t{}", median(&mut decode_samples));
    println!("original_hash\t{}", median(&mut original_hash_samples));
    println!("reference_blake3\t{}", median(&mut reference_hash_samples));
    println!("volatile_copy\t{}", median(&mut volatile_copy_samples));
    println!("input_bytes\t{}", original.len());
    println!("payload_bytes\t{}", payload.len());
}
