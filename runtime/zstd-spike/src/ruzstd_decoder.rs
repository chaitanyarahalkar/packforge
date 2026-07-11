use std::env;
use std::fs;

use ruzstd::decoding::StreamingDecoder;
use ruzstd::io::Read as _;

fn main() {
    let mut arguments = env::args_os().skip(1);
    let input_path = arguments.next().expect("input path");
    let output_path = arguments.next().expect("output path");
    assert!(
        arguments.next().is_none(),
        "expected input and output paths"
    );

    let input = fs::read(input_path).expect("read input");
    let mut decoder = StreamingDecoder::new(input.as_slice()).expect("initialize decoder");
    let mut output = Vec::new();
    decoder.read_to_end(&mut output).expect("decode frame");
    fs::write(output_path, output).expect("write output");
}
