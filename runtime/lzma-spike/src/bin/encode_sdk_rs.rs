use std::fs;
use std::path::PathBuf;

use lzma_sdk_rs::LzmaProps;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let input_path = arguments.next().ok_or("missing input path")?;
    let output_path = arguments.next().ok_or("missing output path")?;
    if arguments.next().is_some() {
        return Err("usage: encode_sdk_rs INPUT OUTPUT".into());
    }
    let input = fs::read(input_path)?;
    let input_size = u32::try_from(input.len())?;
    let properties = LzmaProps::for_level(9, input_size);
    let encoded = lzma_sdk_rs::encode(&input, &properties);
    let decoded = lzma_sdk_rs::decode_raw(
        &encoded,
        &lzma_sdk_rs::decoder_props(&properties),
        input.len(),
    );
    if decoded != input {
        return Err("lzma-sdk-rs round trip mismatch".into());
    }
    let decoder_properties = lzma_sdk_rs::decoder_props(&properties);
    let mut runtime_decoded = vec![0u8; input.len()];
    packforge_runtime_linux_x86_64::lzma::decompress(
        &encoded,
        &decoder_properties,
        &mut runtime_decoded,
    )
    .map_err(|error| format!("runtime decoder failed: {error:?}"))?;
    if runtime_decoded != input {
        return Err("runtime decoder round trip mismatch".into());
    }
    let mut output = Vec::with_capacity(5 + encoded.len());
    output.extend_from_slice(&decoder_properties);
    output.extend_from_slice(&encoded);
    fs::write(output_path, output)?;
    Ok(())
}
