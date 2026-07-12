use std::fs;
use std::io::{BufReader, Cursor};
use std::path::PathBuf;

use lzma_rs::compress::{Options, UnpackedSize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let input_path = arguments.next().ok_or("missing input path")?;
    let output_path = arguments.next().ok_or("missing output path")?;
    if arguments.next().is_some() {
        return Err("usage: encode_lzma_rs INPUT OUTPUT".into());
    }
    let input = fs::read(input_path)?;
    let input_size = u64::try_from(input.len())?;
    let mut reader = BufReader::new(Cursor::new(&input));
    let mut output = Vec::new();
    lzma_rs::lzma_compress_with_options(
        &mut reader,
        &mut output,
        &Options {
            unpacked_size: UnpackedSize::WriteToHeader(Some(input_size)),
        },
    )?;
    fs::write(output_path, output)?;
    Ok(())
}
