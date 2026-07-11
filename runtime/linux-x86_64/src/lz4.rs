//! Bounded decoder for the raw LZ4 block format emitted by the host packer.

/// Decompresses one raw LZ4 block into an exact-size caller-provided output.
///
/// The function rejects truncated lengths, zero or out-of-range match offsets,
/// arithmetic overflow, trailing partial sequences, and output-size mismatch.
#[allow(clippy::result_unit_err)]
pub fn decompress(input: &[u8], output: &mut [u8]) -> Result<(), ()> {
    let mut input_position = 0usize;
    let mut output_position = 0usize;
    while input_position < input.len() {
        let token = input[input_position];
        input_position += 1;
        let literal_length = read_length(input, &mut input_position, usize::from(token >> 4))?;
        let literal_end = input_position.checked_add(literal_length).ok_or(())?;
        let output_literal_end = output_position.checked_add(literal_length).ok_or(())?;
        let literals = input.get(input_position..literal_end).ok_or(())?;
        let output_literals = output
            .get_mut(output_position..output_literal_end)
            .ok_or(())?;
        output_literals.copy_from_slice(literals);
        input_position = literal_end;
        output_position = output_literal_end;
        if input_position == input.len() {
            return (output_position == output.len()).then_some(()).ok_or(());
        }

        let offset_bytes = input.get(input_position..input_position + 2).ok_or(())?;
        let offset = usize::from(u16::from_le_bytes([offset_bytes[0], offset_bytes[1]]));
        input_position += 2;
        if offset == 0 || offset > output_position {
            return Err(());
        }
        let base_match_length = usize::from(token & 0x0f);
        let match_length = read_length(input, &mut input_position, base_match_length)?
            .checked_add(4)
            .ok_or(())?;
        let match_end = output_position.checked_add(match_length).ok_or(())?;
        if match_end > output.len() {
            return Err(());
        }
        for _ in 0..match_length {
            output[output_position] = output[output_position - offset];
            output_position += 1;
        }
    }
    Err(())
}

fn read_length(input: &[u8], position: &mut usize, base: usize) -> Result<usize, ()> {
    if base != 15 {
        return Ok(base);
    }
    let mut length = base;
    loop {
        let extension = usize::from(*input.get(*position).ok_or(())?);
        *position += 1;
        length = length.checked_add(extension).ok_or(())?;
        if extension != 255 {
            return Ok(length);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decompress;

    #[test]
    fn decodes_host_encoder_output_at_boundaries() {
        for length in [1, 15, 16, 63, 64, 255, 1024, 16_384] {
            let input: std::vec::Vec<u8> = (0..length)
                .map(|index| ((index / 7) as u8).wrapping_mul(19))
                .collect();
            let compressed = lz4_flex::block::compress(&input);
            let mut output = std::vec![0u8; input.len()];
            decompress(&compressed, &mut output).unwrap();
            assert_eq!(output, input, "{length}");
        }
    }

    #[test]
    fn rejects_zero_match_offset_and_wrong_output_size() {
        let mut output = [0u8; 4];
        assert!(decompress(&[0x00, 0x00, 0x00], &mut output).is_err());

        let compressed = lz4_flex::block::compress(b"hello hello hello");
        assert!(decompress(&compressed, &mut [0u8; 3]).is_err());
    }
}
