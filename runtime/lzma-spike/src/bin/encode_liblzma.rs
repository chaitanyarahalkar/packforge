use std::fs;
use std::io::{Cursor, Read as _};
use std::path::PathBuf;

use liblzma::read::XzEncoder;
use liblzma::stream::{LzmaOptions, PRESET_EXTREME, Stream};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let input_path = arguments.next().ok_or("missing input path")?;
    let output_path = arguments.next().ok_or("missing output path")?;
    if arguments.next().is_some() {
        return Err("usage: encode_liblzma INPUT OUTPUT".into());
    }
    let input = fs::read(input_path)?;
    let options = LzmaOptions::new_preset(9 | PRESET_EXTREME)?;
    let stream = Stream::new_lzma_encoder(&options)?;
    let mut encoder = XzEncoder::new_stream(Cursor::new(input), stream);
    let mut output = Vec::new();
    encoder.read_to_end(&mut output)?;
    fs::write(output_path, output)?;
    Ok(())
}
