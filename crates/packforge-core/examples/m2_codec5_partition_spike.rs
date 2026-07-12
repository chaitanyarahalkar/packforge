//! Host-only APultra MAIN-stream partition feasibility probe for M2.

use std::{env, fs};

use packforge_codec5_sys::{apultra_compress_bytes, apultra_decompress_bytes, bcj2_encode};

fn boundary(length: usize, index: usize, streams: usize) -> usize {
    if index == streams {
        length
    } else {
        (length * index / streams) & !3
    }
}

fn main() {
    let input = env::args()
        .nth(1)
        .expect("usage: m2_codec5_partition_spike <runtime-image>");
    assert!(env::args().nth(2).is_none(), "too many arguments");
    let runtime = fs::read(input).expect("cannot read runtime image");
    let streams = bcj2_encode(&runtime).expect("BCJ2 split failed");
    let main = &streams[0];
    let whole = apultra_compress_bytes(main).expect("APultra main compression failed");

    println!(
        "streams\tmain_decoded_bytes\twhole_main_compressed_bytes\tsplit_main_compressed_bytes\tmain_delta_bytes"
    );
    for count in [1usize, 2, 4] {
        let mut reconstructed = Vec::with_capacity(main.len());
        let mut compressed = 0usize;
        for index in 0..count {
            let start = boundary(main.len(), index, count);
            let end = boundary(main.len(), index + 1, count);
            assert!(start < end, "empty partition");
            let encoded = apultra_compress_bytes(&main[start..end])
                .expect("APultra partition compression failed");
            let decoded = apultra_decompress_bytes(&encoded, end - start)
                .expect("APultra partition decompression failed");
            assert_eq!(decoded, main[start..end], "partition round trip mismatch");
            reconstructed.extend_from_slice(&decoded);
            compressed = compressed
                .checked_add(encoded.len())
                .expect("compressed size overflow");
        }
        assert_eq!(reconstructed, *main, "MAIN reassembly mismatch");
        let delta = isize::try_from(compressed)
            .and_then(|value| isize::try_from(whole.len()).map(|whole| value - whole))
            .expect("size difference exceeds isize");
        println!(
            "{count}\t{}\t{}\t{compressed}\t{delta}",
            main.len(),
            whole.len()
        );
    }
}
