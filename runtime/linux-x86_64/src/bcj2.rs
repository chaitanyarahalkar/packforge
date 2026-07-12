//! Checked, allocation-free one-shot BCJ2 decoder.

const TOP_VALUE: u32 = 1 << 24;
const BIT_MODEL_TOTAL: u32 = 1 << 11;
const MOVE_BITS: u32 = 5;
const PROBABILITY_COUNT: usize = 258;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Input,
    Output,
    Range,
    Trailing,
}

/// Reconstructs one exact runtime image from canonical BCJ2 streams.
///
/// `jump` is the four-plane transposed representation used by codec 5.
pub fn decode(
    main: &[u8],
    call: &[u8],
    jump: &[u8],
    range_stream: &[u8],
    output: &mut [u8],
) -> Result<(), Error> {
    if !call.len().is_multiple_of(4)
        || !jump.len().is_multiple_of(4)
        || range_stream.len() < 5
    {
        return Err(Error::Input);
    }
    if main
        .len()
        .checked_add(call.len())
        .and_then(|length| length.checked_add(jump.len()))
        != Some(output.len())
    {
        return Err(Error::Output);
    }
    let mut rc_position = 0usize;
    let mut code = 0u32;
    for index in 0..5 {
        let byte = *range_stream.get(rc_position).ok_or(Error::Input)?;
        rc_position += 1;
        if index == 1 && code != 0 {
            return Err(Error::Range);
        }
        code = (code << 8) | u32::from(byte);
    }
    if code == u32::MAX {
        return Err(Error::Range);
    }
    let mut range = u32::MAX;
    let mut probabilities = [BIT_MODEL_TOTAL as u16 / 2; PROBABILITY_COUNT];
    let mut main_position = 0usize;
    let mut call_position = 0usize;
    let mut jump_position = 0usize;
    let mut output_position = 0usize;
    let mut ip = 0u32;
    let mut previous = 0u8;
    while main_position < main.len() {
        normalize(
            &mut range,
            &mut code,
            range_stream,
            &mut rc_position,
        )?;
        let byte = main[main_position];
        main_position += 1;
        *output.get_mut(output_position).ok_or(Error::Output)? = byte;
        output_position += 1;
        ip = ip.wrapping_add(1);
        let marker = matches!(byte, 0xe8 | 0xe9) || (previous == 0x0f && (0x80..=0x8f).contains(&byte));
        let context = previous;
        previous = byte;
        if !marker {
            continue;
        }
        let probability_index = if byte == 0xe8 {
            usize::from(context) + 2
        } else if byte == 0xe9 {
            1
        } else {
            0
        };
        let probability = u32::from(probabilities[probability_index]);
        let bound = (range >> 11) * probability;
        if code < bound {
            range = bound;
            probabilities[probability_index] = (probability
                + ((BIT_MODEL_TOTAL - probability) >> MOVE_BITS)) as u16;
            continue;
        }
        range -= bound;
        code -= bound;
        probabilities[probability_index] = (probability - (probability >> MOVE_BITS)) as u16;
        let absolute = if byte == 0xe8 {
            read_be(call, &mut call_position)?
        } else {
            read_transposed_be(jump, &mut jump_position)?
        };
        let relative = absolute.wrapping_sub(ip.wrapping_add(4));
        let end = output_position.checked_add(4).ok_or(Error::Output)?;
        output
            .get_mut(output_position..end)
            .ok_or(Error::Output)?
            .copy_from_slice(&relative.to_le_bytes());
        output_position = end;
        ip = ip.wrapping_add(4);
        previous = relative.to_le_bytes()[3];
    }
    if output_position != output.len()
        || call_position != call.len()
        || jump_position != jump.len()
        || rc_position != range_stream.len()
        || code != 0
    {
        return Err(Error::Trailing);
    }
    Ok(())
}

fn normalize(
    range: &mut u32,
    code: &mut u32,
    input: &[u8],
    position: &mut usize,
) -> Result<(), Error> {
    if *range < TOP_VALUE {
        *range <<= 8;
        *code = (*code << 8) | u32::from(*input.get(*position).ok_or(Error::Input)?);
        *position += 1;
    }
    Ok(())
}

fn read_be(input: &[u8], position: &mut usize) -> Result<u32, Error> {
    let end = position.checked_add(4).ok_or(Error::Input)?;
    let bytes: [u8; 4] = input
        .get(*position..end)
        .ok_or(Error::Input)?
        .try_into()
        .map_err(|_| Error::Input)?;
    *position = end;
    Ok(u32::from_be_bytes(bytes))
}

fn read_transposed_be(input: &[u8], position: &mut usize) -> Result<u32, Error> {
    let count = input.len() / 4;
    let index = *position / 4;
    if !(*position).is_multiple_of(4) || index >= count {
        return Err(Error::Input);
    }
    *position += 4;
    Ok(u32::from_be_bytes([
        input[index],
        input[count + index],
        input[2 * count + index],
        input[3 * count + index],
    ]))
}

#[cfg(test)]
mod tests {
    use packforge_codec5_sys::bcj2_encode;

    use super::{Error, decode};

    #[test]
    fn matches_pinned_bcj2_encoder_and_rejects_truncation() {
        let mut original: std::vec::Vec<u8> = (0u8..=u8::MAX)
            .cycle()
            .take(65_537)
            .collect();
        for offset in (128..65_000).step_by(32) {
            original[offset] = if offset % 64 == 0 { 0xe8 } else { 0xe9 };
            original[offset + 1..offset + 5]
                .copy_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());
        }
        let streams = bcj2_encode(&original).unwrap();
        let jump = transpose(&streams[2]);
        let mut decoded = std::vec![0u8; original.len()];
        decode(
            &streams[0],
            &streams[1],
            &jump,
            &streams[3],
            &mut decoded,
        )
        .unwrap();
        assert_eq!(decoded, original);

        assert!(matches!(
            decode(
                &streams[0],
                &streams[1],
                &jump,
                &streams[3][..streams[3].len() - 1],
                &mut decoded,
            ),
            Err(Error::Input | Error::Trailing)
        ));
    }

    fn transpose(input: &[u8]) -> std::vec::Vec<u8> {
        let count = input.len() / 4;
        let mut output = std::vec![0u8; input.len()];
        for index in 0..count {
            for byte in 0..4 {
                output[byte * count + index] = input[index * 4 + byte];
            }
        }
        output
    }
}
