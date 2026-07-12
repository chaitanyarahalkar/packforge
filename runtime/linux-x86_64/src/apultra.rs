// Checked, allocation-free decoder for the APultra byte-stream format.

const MIN_MATCH3_OFFSET: usize = 1280;
const MIN_MATCH4_OFFSET: usize = 32_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Input,
    Output,
    Offset,
    Overflow,
    Trailing,
}

struct Bits<'a> {
    input: &'a [u8],
    position: usize,
    byte: u8,
    mask: u8,
}

impl<'a> Bits<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            position: 0,
            byte: 0,
            mask: 0,
        }
    }

    fn byte(&mut self) -> Result<u8, Error> {
        let value = *self.input.get(self.position).ok_or(Error::Input)?;
        self.position += 1;
        Ok(value)
    }

    fn bit(&mut self) -> Result<usize, Error> {
        if self.mask == 0 {
            self.byte = self.byte()?;
            self.mask = 0x80;
        }
        let value = usize::from(self.byte & 0x80 != 0);
        self.byte <<= 1;
        self.mask >>= 1;
        Ok(value)
    }

    fn gamma2(&mut self) -> Result<usize, Error> {
        let mut value = 1usize;
        loop {
            let bit = self.bit()?;
            value = value
                .checked_mul(2)
                .and_then(|value| value.checked_add(bit))
                .ok_or(Error::Overflow)?;
            if self.bit()? == 0 {
                return Ok(value);
            }
        }
    }
}

/// Decodes one exact `APultra` stream into `output`.
///
/// # Errors
///
/// Returns [`Error`] for truncated input, invalid distances, arithmetic
/// overflow, output-length mismatch, or noncanonical trailing bytes.
pub fn decompress(input: &[u8], output: &mut [u8]) -> Result<(), Error> {
    if input.is_empty() || output.is_empty() {
        return Err(Error::Input);
    }
    let mut bits = Bits::new(input);
    output[0] = bits.byte()?;
    let mut position = 1usize;
    let mut match_offset = None;
    let mut follows_literal = 3usize;
    loop {
        if bits.bit()? == 0 {
            let destination = output.get_mut(position).ok_or(Error::Output)?;
            *destination = bits.byte()?;
            position += 1;
            follows_literal = 3;
            continue;
        }
        if bits.bit()? == 0 {
            let encoded_offset = bits.gamma2()?;
            let mut length;
            if encoded_offset >= follows_literal {
                let high = encoded_offset - follows_literal;
                let offset = high
                    .checked_mul(256)
                    .and_then(|value| value.checked_add(usize::from(bits.byte().ok()?)))
                    .ok_or(Error::Overflow)?;
                if offset == 0 {
                    return Err(Error::Offset);
                }
                match_offset = Some(offset);
                length = bits.gamma2()?;
                if !(128..MIN_MATCH4_OFFSET).contains(&offset) {
                    length = length.checked_add(2).ok_or(Error::Overflow)?;
                } else if offset >= MIN_MATCH3_OFFSET {
                    length = length.checked_add(1).ok_or(Error::Overflow)?;
                }
            } else {
                length = bits.gamma2()?;
            }
            follows_literal = 2;
            copy_match(output, &mut position, match_offset.ok_or(Error::Offset)?, length)?;
            continue;
        }
        if bits.bit()? == 0 {
            let command = bits.byte()?;
            if command == 0 {
                break;
            }
            let offset = usize::from(command >> 1);
            let length = usize::from(command & 1) + 2;
            match_offset = Some(offset);
            follows_literal = 2;
            copy_match(output, &mut position, offset, length)?;
            continue;
        }
        let mut offset = 0usize;
        for _ in 0..4 {
            offset = (offset << 1) | bits.bit()?;
        }
        follows_literal = 3;
        let value = if offset == 0 {
            0
        } else {
            *output
                .get(position.checked_sub(offset).ok_or(Error::Offset)?)
                .ok_or(Error::Offset)?
        };
        *output.get_mut(position).ok_or(Error::Output)? = value;
        position += 1;
    }
    if position != output.len() {
        return Err(Error::Output);
    }
    if bits.position != input.len() {
        return Err(Error::Trailing);
    }
    Ok(())
}

fn copy_match(
    output: &mut [u8],
    position: &mut usize,
    offset: usize,
    length: usize,
) -> Result<(), Error> {
    if offset == 0 || offset > *position {
        return Err(Error::Offset);
    }
    let end = position.checked_add(length).ok_or(Error::Overflow)?;
    if end > output.len() {
        return Err(Error::Output);
    }
    while *position < end {
        let source = *position - offset;
        output[*position] = output[source];
        *position += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use packforge_codec5_sys::apultra_compress_bytes;

    use super::{Error, copy_match, decompress};

    #[test]
    fn matches_pinned_apultra_encoder_and_rejects_corruption() {
        let original: std::vec::Vec<u8> = (0u8..=u8::MAX)
            .cycle()
            .take(65_537)
            .map(|value| value.wrapping_mul(17).wrapping_add(3))
            .collect();
        let compressed = apultra_compress_bytes(&original).unwrap();
        let mut decoded = std::vec![0u8; original.len()];
        decompress(&compressed, &mut decoded).unwrap();
        assert_eq!(decoded, original);

        let mut truncated = compressed;
        truncated.pop();
        assert!(matches!(
            decompress(&truncated, &mut decoded),
            Err(Error::Input | Error::Output | Error::Trailing)
        ));
    }

    #[test]
    fn match_copy_handles_short_periods_and_rejects_invalid_ranges() {
        let mut repeated = [0u8; 33];
        repeated[0] = 0xa5;
        let mut position = 1;
        copy_match(&mut repeated, &mut position, 1, 32).unwrap();
        assert_eq!(repeated, [0xa5; 33]);
        assert_eq!(position, repeated.len());

        let mut periodic = *b"abc..............................";
        let mut position = 3;
        copy_match(&mut periodic, &mut position, 3, 30).unwrap();
        assert_eq!(&periodic, b"abcabcabcabcabcabcabcabcabcabcabc");

        let mut output = [0u8; 8];
        assert_eq!(copy_match(&mut output, &mut 0, 1, 1), Err(Error::Offset));
        assert_eq!(copy_match(&mut output, &mut 1, 0, 1), Err(Error::Offset));
        assert_eq!(copy_match(&mut output, &mut 1, 1, 8), Err(Error::Output));
    }
}
