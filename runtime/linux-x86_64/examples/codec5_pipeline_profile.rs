use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use packforge_runtime_linux_x86_64::{apultra, bcj2, hash};

fn median(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn read(prefix: &Path, suffix: &str) -> Vec<u8> {
    fs::read(format!("{}{suffix}", prefix.display())).expect("cannot read codec-5 stream")
}

fn length(prefix: &Path, suffix: &str) -> usize {
    usize::try_from(
        fs::metadata(format!("{}{suffix}", prefix.display()))
            .expect("cannot inspect codec-5 stream")
            .len(),
    )
    .expect("codec-5 stream is too large")
}

fn main() {
    let mut arguments = env::args().skip(1);
    let original_path = arguments
        .next()
        .expect("usage: codec5_pipeline_profile <runtime-image> <stream-prefix> [iterations]");
    let prefix = arguments
        .next()
        .expect("usage: codec5_pipeline_profile <runtime-image> <stream-prefix> [iterations]");
    let iterations = arguments
        .next()
        .map_or(Ok(21usize), |value| value.parse::<usize>())
        .expect("iterations must be a positive integer");
    assert!(iterations > 0, "iterations must be positive");
    assert!(arguments.next().is_none(), "too many arguments");

    let original = fs::read(original_path).expect("cannot read runtime image");
    let expected_hash = hash::hash(&original);
    let prefix = Path::new(&prefix);
    let main_compressed = read(prefix, ".main.apu");
    let call_compressed = read(prefix, ".call.apu");
    let jump_compressed = read(prefix, ".jump.transpose.apu");
    let range_stream = read(prefix, ".rc");
    let mut main = vec![0u8; length(prefix, ".main")];
    let mut call = vec![0u8; length(prefix, ".call")];
    let mut jump = vec![0u8; length(prefix, ".jump.transpose")];
    let mut output = vec![0u8; original.len()];
    let mut main_samples = Vec::with_capacity(iterations);
    let mut call_samples = Vec::with_capacity(iterations);
    let mut jump_samples = Vec::with_capacity(iterations);
    let mut bcj2_samples = Vec::with_capacity(iterations);
    let mut hash_samples = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        apultra::decompress(black_box(&main_compressed), black_box(&mut main))
            .expect("MAIN decode failed");
        main_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        apultra::decompress(black_box(&call_compressed), black_box(&mut call))
            .expect("CALL decode failed");
        call_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        apultra::decompress(black_box(&jump_compressed), black_box(&mut jump))
            .expect("JUMP decode failed");
        jump_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        bcj2::decode(
            black_box(&main),
            black_box(&call),
            black_box(&jump),
            black_box(&range_stream),
            black_box(&mut output),
        )
        .expect("BCJ2 decode failed");
        bcj2_samples.push(start.elapsed().as_nanos());

        let start = Instant::now();
        let actual_hash = hash::hash(black_box(&output));
        hash_samples.push(start.elapsed().as_nanos());
        assert_eq!(actual_hash, expected_hash, "runtime image mismatch");
    }

    println!(
        "{}\t{}\t{}\t{}\t{}",
        median(&mut main_samples),
        median(&mut call_samples),
        median(&mut jump_samples),
        median(&mut bcj2_samples),
        median(&mut hash_samples)
    );
}
