//! Bounded, allocation-free decoder for Packforge's fixed raw LZMA1 profile.

const BIT_MODEL_TOTAL: u32 = 1 << 11;
const MOVE_BITS: u32 = 5;
const PROBABILITY_INITIAL: u16 = 1024;
const TOP_VALUE: u32 = 1 << 24;
const MATCH_LENGTH_MINIMUM: usize = 2;
const STATE_COUNT: usize = 12;
const POSITION_STATE_COUNT: usize = 16;
const POSITION_SLOT_COUNT: usize = 64;
const ALIGN_COUNT: usize = 16;
const FULL_DISTANCE_COUNT: usize = 128;
const LENGTH_LOW_COUNT: usize = 8;
const LENGTH_HIGH_COUNT: usize = 256;
const LITERAL_PROBABILITY_COUNT: usize = 0x300 << 3;
const MINIMUM_DICTIONARY_SIZE: usize = 1 << 12;
const MAXIMUM_DICTIONARY_SIZE: usize = 1 << 26;
const FIXED_PROPERTIES: u8 = 0x5d; // lc=3, lp=0, pb=2

/// Why a raw LZMA1 stream was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The fixed properties or dictionary bound are invalid.
    Properties,
    /// The range-coded input ended before the declared output was produced.
    Truncated,
    /// A match refers before the produced output or beyond the dictionary.
    Distance,
    /// A decoded match would exceed the exact caller-provided output.
    OutputOverflow,
    /// Bytes remain after producing the exact declared output.
    TrailingData,
}

/// Canonical framing facts returned after exact decompression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeReport {
    /// Range-coder flush bytes left after the known output length is reached.
    pub trailing_bytes: u8,
}

struct LengthProbabilities {
    choice: u16,
    choice2: u16,
    low: [[u16; LENGTH_LOW_COUNT]; POSITION_STATE_COUNT],
    middle: [[u16; LENGTH_LOW_COUNT]; POSITION_STATE_COUNT],
    high: [u16; LENGTH_HIGH_COUNT],
}

impl LengthProbabilities {
    const fn new() -> Self {
        Self {
            choice: PROBABILITY_INITIAL,
            choice2: PROBABILITY_INITIAL,
            low: [[PROBABILITY_INITIAL; LENGTH_LOW_COUNT]; POSITION_STATE_COUNT],
            middle: [[PROBABILITY_INITIAL; LENGTH_LOW_COUNT]; POSITION_STATE_COUNT],
            high: [PROBABILITY_INITIAL; LENGTH_HIGH_COUNT],
        }
    }
}

struct Probabilities {
    is_match: [[u16; POSITION_STATE_COUNT]; STATE_COUNT],
    is_rep: [u16; STATE_COUNT],
    is_rep_g0: [u16; STATE_COUNT],
    is_rep_g1: [u16; STATE_COUNT],
    is_rep_g2: [u16; STATE_COUNT],
    is_rep0_long: [[u16; POSITION_STATE_COUNT]; STATE_COUNT],
    position_slot: [[u16; POSITION_SLOT_COUNT]; 4],
    special_position: [u16; FULL_DISTANCE_COUNT],
    align: [u16; ALIGN_COUNT],
    length: LengthProbabilities,
    repeat_length: LengthProbabilities,
    literal: [u16; LITERAL_PROBABILITY_COUNT],
}

impl Probabilities {
    const fn new() -> Self {
        Self {
            is_match: [[PROBABILITY_INITIAL; POSITION_STATE_COUNT]; STATE_COUNT],
            is_rep: [PROBABILITY_INITIAL; STATE_COUNT],
            is_rep_g0: [PROBABILITY_INITIAL; STATE_COUNT],
            is_rep_g1: [PROBABILITY_INITIAL; STATE_COUNT],
            is_rep_g2: [PROBABILITY_INITIAL; STATE_COUNT],
            is_rep0_long: [[PROBABILITY_INITIAL; POSITION_STATE_COUNT]; STATE_COUNT],
            position_slot: [[PROBABILITY_INITIAL; POSITION_SLOT_COUNT]; 4],
            special_position: [PROBABILITY_INITIAL; FULL_DISTANCE_COUNT],
            align: [PROBABILITY_INITIAL; ALIGN_COUNT],
            length: LengthProbabilities::new(),
            repeat_length: LengthProbabilities::new(),
            literal: [PROBABILITY_INITIAL; LITERAL_PROBABILITY_COUNT],
        }
    }
}

struct RangeDecoder<'a> {
    input: &'a [u8],
    position: usize,
    range: u32,
    code: u32,
}

impl<'a> RangeDecoder<'a> {
    fn new(input: &'a [u8]) -> Result<Self, DecodeError> {
        let initial = input.get(..5).ok_or(DecodeError::Truncated)?;
        if initial[0] != 0 {
            return Err(DecodeError::Properties);
        }
        Ok(Self {
            input,
            position: 5,
            range: u32::MAX,
            code: u32::from_be_bytes([initial[1], initial[2], initial[3], initial[4]]),
        })
    }

    fn next_byte(&mut self) -> Result<u32, DecodeError> {
        let byte = *self
            .input
            .get(self.position)
            .ok_or(DecodeError::Truncated)?;
        self.position += 1;
        Ok(u32::from(byte))
    }

    fn normalize(&mut self) -> Result<(), DecodeError> {
        if self.range < TOP_VALUE {
            self.range <<= 8;
            self.code = (self.code << 8) | self.next_byte()?;
        }
        Ok(())
    }

    fn bit(&mut self, probability: &mut u16) -> Result<u32, DecodeError> {
        self.normalize()?;
        let probability_value = u32::from(*probability);
        let bound = (self.range >> 11) * probability_value;
        if self.code < bound {
            self.range = bound;
            *probability =
                (probability_value + ((BIT_MODEL_TOTAL - probability_value) >> MOVE_BITS)) as u16;
            Ok(0)
        } else {
            self.range -= bound;
            self.code -= bound;
            *probability = (probability_value - (probability_value >> MOVE_BITS)) as u16;
            Ok(1)
        }
    }

    fn direct_bits(&mut self, count: u32) -> Result<u32, DecodeError> {
        let mut result = 0u32;
        for _ in 0..count {
            self.normalize()?;
            self.range >>= 1;
            self.code = self.code.wrapping_sub(self.range);
            let mask = 0u32.wrapping_sub(self.code >> 31);
            self.code = self.code.wrapping_add(self.range & mask);
            result = (result << 1).wrapping_add(mask.wrapping_add(1));
        }
        Ok(result)
    }

    fn tree(&mut self, probabilities: &mut [u16], bits: u32) -> Result<u32, DecodeError> {
        let mut symbol = 1u32;
        for _ in 0..bits {
            symbol = (symbol << 1) | self.bit(&mut probabilities[symbol as usize])?;
        }
        Ok(symbol - (1 << bits))
    }

    fn reverse_tree(&mut self, probabilities: &mut [u16], bits: u32) -> Result<u32, DecodeError> {
        let mut node = 1u32;
        let mut symbol = 0u32;
        for index in 0..bits {
            let bit = self.bit(&mut probabilities[node as usize])?;
            node = (node << 1) | bit;
            symbol |= bit << index;
        }
        Ok(symbol)
    }
}

fn decode_length(
    decoder: &mut RangeDecoder<'_>,
    probabilities: &mut LengthProbabilities,
    position_state: usize,
) -> Result<u32, DecodeError> {
    if decoder.bit(&mut probabilities.choice)? == 0 {
        decoder.tree(&mut probabilities.low[position_state], 3)
    } else if decoder.bit(&mut probabilities.choice2)? == 0 {
        Ok(8 + decoder.tree(&mut probabilities.middle[position_state], 3)?)
    } else {
        Ok(16 + decoder.tree(&mut probabilities.high, 8)?)
    }
}

fn dictionary_size(properties: &[u8; 5]) -> Result<usize, DecodeError> {
    if properties[0] != FIXED_PROPERTIES {
        return Err(DecodeError::Properties);
    }
    let size = u32::from_le_bytes([properties[1], properties[2], properties[3], properties[4]]);
    let size = usize::try_from(size).map_err(|_| DecodeError::Properties)?;
    if !(MINIMUM_DICTIONARY_SIZE..=MAXIMUM_DICTIONARY_SIZE).contains(&size) {
        return Err(DecodeError::Properties);
    }
    Ok(size)
}

/// Decodes one raw LZMA1 stream into an exact-size output buffer.
///
/// The supported profile is fixed to `lc=3`, `lp=0`, `pb=2`, a 4 KiB through
/// 64 MiB dictionary, no end marker, and a caller-known output length.
pub fn decompress(
    input: &[u8],
    properties: &[u8; 5],
    output: &mut [u8],
) -> Result<DecodeReport, DecodeError> {
    if output.is_empty() {
        return Err(DecodeError::Properties);
    }
    let dictionary_size = dictionary_size(properties)?;
    let mut probabilities = Probabilities::new();
    let mut decoder = RangeDecoder::new(input)?;
    let mut output_position = 0usize;
    let mut state = 0usize;
    let mut repeats = [1usize; 4];

    while output_position < output.len() {
        let position_state = output_position & 3;
        if decoder.bit(&mut probabilities.is_match[state][position_state])? == 0 {
            let previous = output_position
                .checked_sub(1)
                .map_or(0, |position| output[position]);
            let literal_state = usize::from(previous >> 5);
            let table_start = 0x300 * literal_state;
            let table = &mut probabilities.literal[table_start..table_start + 0x300];
            let mut symbol = 1u32;
            if state < 7 {
                while symbol < 0x100 {
                    symbol = (symbol << 1) | decoder.bit(&mut table[symbol as usize])?;
                }
            } else {
                validate_distance(repeats[0], dictionary_size, output_position)?;
                let mut match_byte = u32::from(output[output_position - repeats[0]]);
                let mut offset = 0x100u32;
                while symbol < 0x100 {
                    match_byte <<= 1;
                    let match_bit = offset;
                    offset &= match_byte;
                    let index = (offset + match_bit + symbol) as usize;
                    let bit = decoder.bit(&mut table[index])?;
                    symbol = (symbol << 1) | bit;
                    if bit == 0 {
                        offset ^= match_bit;
                    }
                }
            }
            output[output_position] = (symbol & 0xff) as u8;
            output_position += 1;
            state = if state < 4 {
                0
            } else if state < 10 {
                state - 3
            } else {
                state - 6
            };
            continue;
        }

        let length_symbol;
        if decoder.bit(&mut probabilities.is_rep[state])? == 0 {
            repeats[3] = repeats[2];
            repeats[2] = repeats[1];
            repeats[1] = repeats[0];
            length_symbol = decode_length(&mut decoder, &mut probabilities.length, position_state)?;
            state = if state < 7 { 7 } else { 10 };
            let length_state = length_symbol.min(3) as usize;
            let position_slot = decoder.tree(&mut probabilities.position_slot[length_state], 6)?;
            let distance = decode_distance(&mut decoder, &mut probabilities, position_slot)?;
            if distance == u32::MAX {
                return Err(DecodeError::OutputOverflow);
            }
            repeats[0] = usize::try_from(distance)
                .ok()
                .and_then(|distance| distance.checked_add(1))
                .ok_or(DecodeError::Distance)?;
        } else {
            if decoder.bit(&mut probabilities.is_rep_g0[state])? == 0 {
                if decoder.bit(&mut probabilities.is_rep0_long[state][position_state])? == 0 {
                    validate_distance(repeats[0], dictionary_size, output_position)?;
                    output[output_position] = output[output_position - repeats[0]];
                    output_position += 1;
                    state = if state < 7 { 9 } else { 11 };
                    continue;
                }
            } else {
                let distance = if decoder.bit(&mut probabilities.is_rep_g1[state])? == 0 {
                    repeats[1]
                } else if decoder.bit(&mut probabilities.is_rep_g2[state])? == 0 {
                    let distance = repeats[2];
                    repeats[2] = repeats[1];
                    distance
                } else {
                    let distance = repeats[3];
                    repeats[3] = repeats[2];
                    repeats[2] = repeats[1];
                    distance
                };
                repeats[1] = repeats[0];
                repeats[0] = distance;
            }
            length_symbol = decode_length(
                &mut decoder,
                &mut probabilities.repeat_length,
                position_state,
            )?;
            state = if state < 7 { 8 } else { 11 };
        }

        validate_distance(repeats[0], dictionary_size, output_position)?;
        let length = usize::try_from(length_symbol)
            .ok()
            .and_then(|length| length.checked_add(MATCH_LENGTH_MINIMUM))
            .ok_or(DecodeError::OutputOverflow)?;
        let end = output_position
            .checked_add(length)
            .ok_or(DecodeError::OutputOverflow)?;
        if end > output.len() {
            return Err(DecodeError::OutputOverflow);
        }
        while output_position < end {
            output[output_position] = output[output_position - repeats[0]];
            output_position += 1;
        }
    }

    let trailing_bytes = input.len().saturating_sub(decoder.position);
    if trailing_bytes > 5 {
        return Err(DecodeError::TrailingData);
    }
    Ok(DecodeReport {
        trailing_bytes: u8::try_from(trailing_bytes).map_err(|_| DecodeError::TrailingData)?,
    })
}

fn validate_distance(
    distance: usize,
    dictionary_size: usize,
    output_position: usize,
) -> Result<(), DecodeError> {
    if distance == 0 || distance > dictionary_size || distance > output_position {
        return Err(DecodeError::Distance);
    }
    Ok(())
}

fn decode_distance(
    decoder: &mut RangeDecoder<'_>,
    probabilities: &mut Probabilities,
    position_slot: u32,
) -> Result<u32, DecodeError> {
    if position_slot < 4 {
        return Ok(position_slot);
    }
    let direct_bits = (position_slot >> 1) - 1;
    let mut distance = 2 | (position_slot & 1);
    if position_slot < 14 {
        distance <<= direct_bits;
        let mut scale = 1u32;
        let mut node = distance + 1;
        let mut remaining = direct_bits;
        loop {
            let bit = decoder.bit(&mut probabilities.special_position[node as usize])?;
            if bit == 0 {
                node += scale;
                scale += scale;
            } else {
                scale += scale;
                node += scale;
            }
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        Ok(node - scale)
    } else {
        distance <<= direct_bits;
        let high = decoder.direct_bits(direct_bits - 4)? << 4;
        distance = distance.wrapping_add(high);
        Ok(distance.wrapping_add(decoder.reverse_tree(&mut probabilities.align, 4)?))
    }
}

#[cfg(test)]
mod tests {
    use std::vec;
    use std::vec::Vec;

    use super::{DecodeError, decompress};

    fn source(length: usize) -> Vec<u8> {
        (0..length)
            .map(|index| ((index / 11) as u8).wrapping_mul(29) ^ (index as u8).rotate_left(3))
            .collect()
    }

    #[test]
    fn decodes_sdk_encoder_at_boundaries() {
        for length in [1, 2, 15, 16, 255, 4096, 16_384] {
            let input = source(length);
            let properties = lzma_sdk_rs::LzmaProps::for_level(9, length as u32);
            let encoded = lzma_sdk_rs::encode(&input, &properties);
            let decoder_properties = lzma_sdk_rs::decoder_props(&properties);
            let mut output = vec![0u8; input.len()];
            let report = decompress(&encoded, &decoder_properties, &mut output).unwrap();
            assert!(report.trailing_bytes <= 5);
            assert_eq!(output, input, "{length}");
        }
    }

    #[test]
    fn rejects_properties_truncation_distance_and_output_mismatch() {
        let input = source(4096);
        let properties = lzma_sdk_rs::LzmaProps::for_level(9, input.len() as u32);
        let encoded = lzma_sdk_rs::encode(&input, &properties);
        let decoder_properties = lzma_sdk_rs::decoder_props(&properties);

        let mut output = vec![0u8; input.len()];
        let mut invalid_properties = decoder_properties;
        invalid_properties[0] = 0;
        assert_eq!(
            decompress(&encoded, &invalid_properties, &mut output),
            Err(DecodeError::Properties)
        );
        assert_eq!(
            decompress(&encoded[..4], &decoder_properties, &mut output),
            Err(DecodeError::Truncated)
        );
        assert!(decompress(&encoded, &decoder_properties, &mut output[..100]).is_err());

        let mut corrupt = encoded;
        corrupt[0] = 1;
        assert_eq!(
            decompress(&corrupt, &decoder_properties, &mut output),
            Err(DecodeError::Properties)
        );
    }

    #[test]
    fn rejects_noncanonical_trailing_data() {
        let input = source(1024);
        let properties = lzma_sdk_rs::LzmaProps::for_level(9, input.len() as u32);
        let mut encoded = lzma_sdk_rs::encode(&input, &properties);
        let mut output = vec![0u8; input.len()];
        let properties = lzma_sdk_rs::decoder_props(&properties);
        let canonical = decompress(&encoded, &properties, &mut output).unwrap();
        encoded.push(0);
        match decompress(&encoded, &properties, &mut output) {
            Ok(appended) => assert_eq!(
                appended.trailing_bytes,
                canonical.trailing_bytes + 1,
                "v2 metadata can reject a changed trailing-byte count"
            ),
            Err(error) => assert_eq!(error, DecodeError::TrailingData),
        }
    }
}
