use std::env;
use std::fs;

fn main() {
    let mut arguments = env::args_os().skip(1);
    let input_path = arguments.next().expect("input path");
    let output_path = arguments.next().expect("output path");
    assert!(
        arguments.next().is_none(),
        "expected input and output paths"
    );
    let input = fs::read(input_path).expect("read input");
    fs::write(output_path, input).expect("write output");
}
