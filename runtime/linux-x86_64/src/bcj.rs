//! Reversible x86 BCJ decoding for codec 4.

const ALLOWED: [bool; 8] = [true, true, true, false, true, false, false, false];
const BIT_NUMBER: [usize; 8] = [0, 1, 2, 2, 3, 3, 3, 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

pub fn decode(bytes: &mut [u8]) -> Result<(), DecodeError> {
    if bytes.len() <= 4 {
        return Ok(());
    }
    let limit = bytes.len() - 4;
    let mut position = 0usize;
    let mut previous_position = usize::MAX;
    let mut previous_mask = 0usize;
    while position < limit {
        if bytes[position] & 0xfe != 0xe8 {
            position += 1;
            continue;
        }
        previous_position = position.wrapping_sub(previous_position);
        if previous_position <= 3 {
            previous_mask = (previous_mask << (previous_position - 1)) & 7;
            if previous_mask != 0 {
                let byte = bytes[position + 4 - BIT_NUMBER[previous_mask]];
                if !ALLOWED[previous_mask] || matches!(byte, 0 | 0xff) {
                    previous_position = position;
                    previous_mask = (previous_mask << 1) | 1;
                    position += 1;
                    continue;
                }
            }
        } else {
            previous_mask = 0;
        }
        previous_position = position;
        if matches!(bytes[position + 4], 0 | 0xff) {
            let mut source = u32::from_le_bytes(
                bytes[position + 1..position + 5]
                    .try_into()
                    .map_err(|_| DecodeError)?,
            );
            let mut destination;
            loop {
                let program_counter = u32::try_from(position)
                    .map_err(|_| DecodeError)?
                    .wrapping_add(5);
                destination = source.wrapping_sub(program_counter);
                if previous_mask == 0 {
                    break;
                }
                let shift =
                    u32::try_from(BIT_NUMBER[previous_mask] * 8).map_err(|_| DecodeError)?;
                let byte = (destination >> (24 - shift)).to_le_bytes()[0];
                if !matches!(byte, 0 | 0xff) {
                    break;
                }
                source = destination ^ ((1u32 << (32 - shift)) - 1);
            }
            destination &= 0x01ff_ffff;
            destination |= 0u32.wrapping_sub(destination & 0x0100_0000);
            bytes[position + 1..position + 5].copy_from_slice(&destination.to_le_bytes());
            position += 5;
        } else {
            previous_mask = (previous_mask << 1) | 1;
            position += 1;
        }
    }
    Ok(())
}
