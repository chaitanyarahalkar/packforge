use std::env;
use std::fs;

fn main() {
    let mut arguments = env::args().skip(1);
    let input_path = arguments
        .next()
        .expect("usage: m2_lzma_encode INPUT OUTPUT REDUCE_SIZE");
    let output_path = arguments
        .next()
        .expect("usage: m2_lzma_encode INPUT OUTPUT REDUCE_SIZE");
    let reduce_size: u32 = arguments
        .next()
        .expect("usage: m2_lzma_encode INPUT OUTPUT REDUCE_SIZE")
        .parse()
        .expect("invalid reduce size");
    assert!(arguments.next().is_none(), "unexpected argument");

    let input = fs::read(input_path).expect("cannot read input");
    let properties = lzma_sdk_rs::LzmaProps::for_level(9, reduce_size);
    let encoded = lzma_sdk_rs::encode(&input, &properties);
    let decoder_properties = lzma_sdk_rs::decoder_props(&properties);
    let mut decoded = vec![0u8; input.len()];
    packforge_lzma_decoder::decompress(&encoded, &decoder_properties, &mut decoded)
        .expect("reference decode failed");
    assert_eq!(decoded, input, "reference decode differs");
    fs::write(output_path, encoded).expect("cannot write output");
}
