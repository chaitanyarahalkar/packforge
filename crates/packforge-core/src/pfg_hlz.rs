//! Deterministic host-side `PFG-HLZ/1` feasibility codec.
//!
//! `PFG-HLZ/1` owns its stream grammar. It encodes a deterministic 64 KiB LZ
//! parse with an input-derived canonical codebook. The first 145 bytes contain
//! two four-bit code lengths per byte for 256 literal symbols, 17 match-length
//! classes, and 17 distance classes. The remaining MSB-first bit stream has no
//! terminator: its authenticated wrapper supplies the exact decoded length.
//! Remaining bits in the final byte must be zero padding.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;

const LITERAL_SYMBOLS: usize = 256;
const LENGTH_CLASSES: usize = 17;
const DISTANCE_CLASSES: usize = 17;
const LENGTH_BASE: usize = LITERAL_SYMBOLS;
const DISTANCE_BASE: usize = LENGTH_BASE + LENGTH_CLASSES;
const SYMBOLS: usize = DISTANCE_BASE + DISTANCE_CLASSES;
const HEADER_BYTES: usize = SYMBOLS.div_ceil(2);
const MAX_CODE_LENGTH: usize = 15;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 65_602;
const WINDOW: usize = 65_536;
const HASH_SIZE: usize = 1 << 16;
const MAX_CHAIN: usize = 64;
const NO_POSITION: usize = usize::MAX;

/// A malformed `PFG-HLZ/1` stream failed bounded decoder validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The stream does not contain the fixed code-length table.
    TruncatedHeader,
    /// A code length exceeds the documented 15-bit bound.
    CodeLengthOutOfRange,
    /// The canonical code lengths describe an oversubscribed tree.
    OversubscribedCodebook,
    /// The bit stream ends while a symbol or extra value is incomplete.
    TruncatedBitstream,
    /// No canonical code matches the next bits of the stream.
    InvalidCode,
    /// A distance class appears where a literal or length class is required.
    UnexpectedSymbol,
    /// A copy points before the beginning of already-produced output.
    DistanceUnderflow,
    /// A literal or copy would exceed the authenticated decoded length.
    OutputOverflow,
    /// The stream ended before the authenticated decoded length was reached.
    OutputUnderflow,
    /// Complete trailing bytes remain after the exact decoded output.
    TrailingData,
    /// The unused suffix of the final bitstream byte is nonzero.
    NonzeroPadding,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader => formatter.write_str("PFG-HLZ code-length table is truncated"),
            Self::CodeLengthOutOfRange => {
                formatter.write_str("PFG-HLZ code length exceeds 15 bits")
            }
            Self::OversubscribedCodebook => {
                formatter.write_str("PFG-HLZ code-length table is oversubscribed")
            }
            Self::TruncatedBitstream => formatter.write_str("PFG-HLZ bit stream is truncated"),
            Self::InvalidCode => formatter.write_str("PFG-HLZ bit stream has no matching code"),
            Self::UnexpectedSymbol => {
                formatter.write_str("PFG-HLZ symbol is invalid in this position")
            }
            Self::DistanceUnderflow => {
                formatter.write_str("PFG-HLZ copy distance underflows output")
            }
            Self::OutputOverflow => formatter.write_str("PFG-HLZ stream exceeds decoded length"),
            Self::OutputUnderflow => {
                formatter.write_str("PFG-HLZ stream ends before decoded length")
            }
            Self::TrailingData => formatter.write_str("PFG-HLZ stream has trailing bytes"),
            Self::NonzeroPadding => formatter.write_str("PFG-HLZ stream has nonzero bit padding"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The deterministic encoder could not represent its input under the fixed
/// `PFG-HLZ/1` code-length limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// The generated canonical codebook requires a code longer than 15 bits.
    CodeLengthOutOfRange,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CodeLengthOutOfRange => {
                formatter.write_str("PFG-HLZ codebook requires more than 15 bits")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

#[derive(Debug, Clone, Copy)]
enum Token {
    Literal(u8),
    Copy { length: usize, distance: usize },
}

#[derive(Debug)]
struct Node {
    left: Option<usize>,
    right: Option<usize>,
    symbol: Option<usize>,
}

struct Codebook {
    lengths: [u8; SYMBOLS],
    codes: [u16; SYMBOLS],
    first_code: [u16; MAX_CODE_LENGTH + 1],
    counts: [u16; MAX_CODE_LENGTH + 1],
    symbols_by_length: [Vec<u16>; MAX_CODE_LENGTH + 1],
}

impl Codebook {
    fn from_frequencies(frequencies: &[u64; SYMBOLS]) -> Result<Self, EncodeError> {
        let mut heap = BinaryHeap::new();
        let mut nodes = Vec::with_capacity(SYMBOLS * 2);
        for (symbol, frequency) in frequencies.iter().copied().enumerate() {
            if frequency == 0 {
                continue;
            }
            let index = nodes.len();
            nodes.push(Node {
                left: None,
                right: None,
                symbol: Some(symbol),
            });
            heap.push(Reverse((frequency, index)));
        }

        let mut lengths = [0u8; SYMBOLS];
        if heap.is_empty() {
            return Ok(Self::from_lengths(&lengths).expect("empty codebook is valid"));
        }
        if heap.len() == 1 {
            let Reverse((_, index)) = heap.pop().expect("one node exists");
            let symbol = nodes[index].symbol.expect("leaf node has a symbol");
            lengths[symbol] = 1;
            return Self::from_lengths(&lengths).map_err(|_| EncodeError::CodeLengthOutOfRange);
        }

        while heap.len() > 1 {
            let Reverse((left_frequency, left)) = heap.pop().expect("two nodes exist");
            let Reverse((right_frequency, right)) = heap.pop().expect("two nodes exist");
            let parent = nodes.len();
            nodes.push(Node {
                left: Some(left),
                right: Some(right),
                symbol: None,
            });
            heap.push(Reverse((left_frequency + right_frequency, parent)));
        }

        let Reverse((_, root)) = heap.pop().expect("root exists");
        let mut pending = vec![(root, 0usize)];
        while let Some((index, depth)) = pending.pop() {
            let node = &nodes[index];
            if let Some(symbol) = node.symbol {
                if depth == 0 || depth > MAX_CODE_LENGTH {
                    return Err(EncodeError::CodeLengthOutOfRange);
                }
                lengths[symbol] = u8::try_from(depth).expect("code depth is bounded");
            } else {
                let left = node.left.expect("interior node has a left child");
                let right = node.right.expect("interior node has a right child");
                pending.push((right, depth + 1));
                pending.push((left, depth + 1));
            }
        }
        Self::from_lengths(&lengths).map_err(|_| EncodeError::CodeLengthOutOfRange)
    }

    fn from_lengths(lengths: &[u8; SYMBOLS]) -> Result<Self, DecodeError> {
        let mut counts = [0u16; MAX_CODE_LENGTH + 1];
        for length in lengths.iter().copied() {
            if usize::from(length) > MAX_CODE_LENGTH {
                return Err(DecodeError::CodeLengthOutOfRange);
            }
            if length != 0 {
                counts[usize::from(length)] += 1;
            }
        }

        let mut first_code = [0u16; MAX_CODE_LENGTH + 1];
        let mut next_code = [0u16; MAX_CODE_LENGTH + 1];
        let mut code = 0u32;
        for bits in 1..=MAX_CODE_LENGTH {
            code = (code + u32::from(counts[bits - 1])) << 1;
            if code + u32::from(counts[bits]) > (1u32 << bits) {
                return Err(DecodeError::OversubscribedCodebook);
            }
            first_code[bits] = u16::try_from(code).expect("15-bit code fits u16");
            next_code[bits] = first_code[bits];
        }

        let mut codes = [0u16; SYMBOLS];
        let mut symbols_by_length: [Vec<u16>; MAX_CODE_LENGTH + 1] =
            std::array::from_fn(|_| Vec::new());
        for (symbol, length) in lengths.iter().copied().enumerate() {
            if length == 0 {
                continue;
            }
            let index = usize::from(length);
            codes[symbol] = next_code[index];
            next_code[index] += 1;
            symbols_by_length[index].push(u16::try_from(symbol).expect("symbol fits u16"));
        }
        Ok(Self {
            lengths: *lengths,
            codes,
            first_code,
            counts,
            symbols_by_length,
        })
    }

    fn write_symbol(&self, writer: &mut BitWriter, symbol: usize) {
        writer.write_bits(u32::from(self.codes[symbol]), self.lengths[symbol]);
    }

    fn read_symbol(&self, reader: &mut BitReader<'_>) -> Result<usize, DecodeError> {
        let mut code = 0u16;
        for length in 1..=MAX_CODE_LENGTH {
            code = (code << 1) | u16::from(reader.read_bit()?);
            let count = self.counts[length];
            let first = self.first_code[length];
            if count != 0 && code >= first && code < first + count {
                let offset = usize::from(code - first);
                return Ok(usize::from(self.symbols_by_length[length][offset]));
            }
        }
        Err(DecodeError::InvalidCode)
    }
}

struct BitWriter {
    bytes: Vec<u8>,
    current: u8,
    used: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            current: 0,
            used: 0,
        }
    }

    fn write_bits(&mut self, value: u32, bit_count: u8) {
        for shift in (0..bit_count).rev() {
            let bit = u8::try_from((value >> shift) & 1).expect("single bit fits u8");
            self.current = (self.current << 1) | bit;
            self.used += 1;
            if self.used == 8 {
                self.bytes.push(self.current);
                self.current = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used != 0 {
            self.current <<= 8 - self.used;
            self.bytes.push(self.current);
        }
        self.bytes
    }
}

struct BitReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_bit(&mut self) -> Result<u8, DecodeError> {
        let byte_index = self.offset / 8;
        let bit_index = self.offset % 8;
        let byte = *self
            .bytes
            .get(byte_index)
            .ok_or(DecodeError::TruncatedBitstream)?;
        self.offset += 1;
        Ok((byte >> (7 - bit_index)) & 1)
    }

    fn read_bits(&mut self, count: usize) -> Result<usize, DecodeError> {
        let mut value = 0usize;
        for _ in 0..count {
            value = (value << 1) | usize::from(self.read_bit()?);
        }
        Ok(value)
    }

    fn finish(&self) -> Result<(), DecodeError> {
        let byte_index = self.offset / 8;
        let bit_index = self.offset % 8;
        if bit_index == 0 {
            return if byte_index == self.bytes.len() {
                Ok(())
            } else {
                Err(DecodeError::TrailingData)
            };
        }
        if byte_index + 1 != self.bytes.len() {
            return Err(DecodeError::TrailingData);
        }
        let remaining_mask = (1u8 << (8 - bit_index)) - 1;
        if self.bytes[byte_index] & remaining_mask == 0 {
            Ok(())
        } else {
            Err(DecodeError::NonzeroPadding)
        }
    }
}

/// Deterministically encodes `input` as a complete `PFG-HLZ/1` stream.
///
/// # Errors
///
/// Returns [`EncodeError`] when the input-derived codebook cannot fit the
/// documented maximum code length.
pub fn encode(input: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let tokens = tokenize(input);
    let mut frequencies = [0u64; SYMBOLS];
    for token in &tokens {
        match *token {
            Token::Literal(byte) => frequencies[usize::from(byte)] += 1,
            Token::Copy { length, distance } => {
                let (length_class, _) = class_for_value(length - 2);
                let (distance_class, _) = class_for_value(distance);
                frequencies[LENGTH_BASE + length_class] += 1;
                frequencies[DISTANCE_BASE + distance_class] += 1;
            }
        }
    }
    let codebook = Codebook::from_frequencies(&frequencies)?;
    let mut output = encode_lengths(&codebook.lengths);
    let mut writer = BitWriter::new();
    for token in tokens {
        match token {
            Token::Literal(byte) => codebook.write_symbol(&mut writer, usize::from(byte)),
            Token::Copy { length, distance } => {
                let (length_class, length_extra) = class_for_value(length - 2);
                codebook.write_symbol(&mut writer, LENGTH_BASE + length_class);
                let length_bits =
                    u8::try_from(length_class).map_err(|_| EncodeError::CodeLengthOutOfRange)?;
                writer.write_bits(
                    u32::try_from(length_extra).map_err(|_| EncodeError::CodeLengthOutOfRange)?,
                    length_bits,
                );
                let (distance_class, distance_extra) = class_for_value(distance);
                codebook.write_symbol(&mut writer, DISTANCE_BASE + distance_class);
                let distance_bits =
                    u8::try_from(distance_class).map_err(|_| EncodeError::CodeLengthOutOfRange)?;
                writer.write_bits(
                    u32::try_from(distance_extra).map_err(|_| EncodeError::CodeLengthOutOfRange)?,
                    distance_bits,
                );
            }
        }
    }
    output.extend(writer.finish());
    Ok(output)
}

/// Decodes one complete `PFG-HLZ/1` stream to its authenticated output length.
///
/// # Errors
///
/// Returns [`DecodeError`] when any stream range, codebook, bit sequence, or
/// copy range is invalid for the supplied decoded length.
pub fn decode(input: &[u8], expected_length: usize) -> Result<Vec<u8>, DecodeError> {
    let lengths = decode_lengths(input)?;
    let codebook = Codebook::from_lengths(&lengths)?;
    let mut reader = BitReader::new(&input[HEADER_BYTES..]);
    let mut output = Vec::with_capacity(expected_length);
    while output.len() < expected_length {
        let symbol = codebook.read_symbol(&mut reader)?;
        if symbol < LITERAL_SYMBOLS {
            append_literal(
                &mut output,
                u8::try_from(symbol).map_err(|_| DecodeError::UnexpectedSymbol)?,
                expected_length,
            )?;
            continue;
        }
        if !(LENGTH_BASE..DISTANCE_BASE).contains(&symbol) {
            return Err(DecodeError::UnexpectedSymbol);
        }
        let length_class = symbol - LENGTH_BASE;
        let length = value_from_class(length_class, reader.read_bits(length_class)?) + 2;
        let distance_symbol = codebook.read_symbol(&mut reader)?;
        if !(DISTANCE_BASE..SYMBOLS).contains(&distance_symbol) {
            return Err(DecodeError::UnexpectedSymbol);
        }
        let distance_class = distance_symbol - DISTANCE_BASE;
        let distance = value_from_class(distance_class, reader.read_bits(distance_class)?);
        append_copy(&mut output, length, distance, expected_length)?;
    }
    reader.finish()?;
    Ok(output)
}

fn encode_lengths(lengths: &[u8; SYMBOLS]) -> Vec<u8> {
    let mut output = Vec::with_capacity(HEADER_BYTES);
    for pair in lengths.chunks(2) {
        let low = pair[0];
        let high = pair.get(1).copied().unwrap_or(0);
        output.push(low | (high << 4));
    }
    output
}

fn decode_lengths(input: &[u8]) -> Result<[u8; SYMBOLS], DecodeError> {
    let header = input
        .get(..HEADER_BYTES)
        .ok_or(DecodeError::TruncatedHeader)?;
    let mut lengths = [0u8; SYMBOLS];
    for (index, byte) in header.iter().copied().enumerate() {
        lengths[index * 2] = byte & 0x0f;
        if index * 2 + 1 < SYMBOLS {
            lengths[index * 2 + 1] = byte >> 4;
        }
    }
    Ok(lengths)
}

fn tokenize(input: &[u8]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut heads = vec![NO_POSITION; HASH_SIZE];
    let mut previous = vec![NO_POSITION; input.len()];
    let mut position = 0;
    while position < input.len() {
        let (length, distance) = find_match(input, position, &heads, &previous);
        if length < MIN_MATCH {
            tokens.push(Token::Literal(input[position]));
            insert_position(input, position, &mut heads, &mut previous);
            position += 1;
            continue;
        }
        tokens.push(Token::Copy { length, distance });
        let end = position + length;
        while position < end {
            insert_position(input, position, &mut heads, &mut previous);
            position += 1;
        }
    }
    tokens
}

fn find_match(
    input: &[u8],
    position: usize,
    heads: &[usize],
    previous: &[usize],
) -> (usize, usize) {
    if input.len() - position < MIN_MATCH {
        return (0, 0);
    }
    let mut candidate = heads[hash_at(input, position)];
    let mut depth = 0;
    let mut best_length = 0;
    let mut best_distance = 0;
    let maximum_length = (input.len() - position).min(MAX_MATCH);
    while candidate != NO_POSITION && depth < MAX_CHAIN {
        let distance = position - candidate;
        if distance > WINDOW {
            break;
        }
        if input[candidate] == input[position]
            && input[candidate + 1] == input[position + 1]
            && input[candidate + 2] == input[position + 2]
        {
            let mut length = MIN_MATCH;
            while length < maximum_length && input[candidate + length] == input[position + length] {
                length += 1;
            }
            if length > best_length {
                best_length = length;
                best_distance = distance;
                if length == maximum_length {
                    break;
                }
            }
        }
        candidate = previous[candidate];
        depth += 1;
    }
    (best_length, best_distance)
}

fn insert_position(input: &[u8], position: usize, heads: &mut [usize], previous: &mut [usize]) {
    if input.len() - position < MIN_MATCH {
        return;
    }
    let slot = hash_at(input, position);
    previous[position] = heads[slot];
    heads[slot] = position;
}

fn hash_at(input: &[u8], position: usize) -> usize {
    let value = (usize::from(input[position]) << 16)
        ^ (usize::from(input[position + 1]) << 8)
        ^ usize::from(input[position + 2]);
    value.wrapping_mul(0x9e37) & (HASH_SIZE - 1)
}

fn class_for_value(value: usize) -> (usize, usize) {
    let mut class = 0;
    while class + 1 < LENGTH_CLASSES && (1usize << (class + 1)) <= value {
        class += 1;
    }
    let base = 1usize << class;
    (class, value - base)
}

fn value_from_class(class: usize, extra: usize) -> usize {
    (1usize << class) + extra
}

fn append_literal(
    output: &mut Vec<u8>,
    byte: u8,
    expected_length: usize,
) -> Result<(), DecodeError> {
    if output.len() == expected_length {
        return Err(DecodeError::OutputOverflow);
    }
    output.push(byte);
    Ok(())
}

fn append_copy(
    output: &mut Vec<u8>,
    length: usize,
    distance: usize,
    expected_length: usize,
) -> Result<(), DecodeError> {
    if distance == 0 || distance > output.len() {
        return Err(DecodeError::DistanceUnderflow);
    }
    let end = output
        .len()
        .checked_add(length)
        .ok_or(DecodeError::OutputOverflow)?;
    if end > expected_length {
        return Err(DecodeError::OutputOverflow);
    }
    for _ in 0..length {
        let source = output.len() - distance;
        output.push(output[source]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DecodeError, HEADER_BYTES, decode, encode};

    #[test]
    fn deterministic_round_trip_covers_literal_and_copy_classes() {
        let mut input = Vec::new();
        for index in 0..4096 {
            input.push(u8::try_from(index % 251).unwrap());
        }
        input.extend(std::iter::repeat_n(0xa5, 70_000));
        let prefix = input[..8192].to_vec();
        input.extend_from_slice(&prefix);

        let first = encode(&input).unwrap();
        let second = encode(&input).unwrap();
        assert_eq!(first, second);
        assert!(first.len() >= HEADER_BYTES);
        assert_eq!(decode(&first, input.len()).unwrap(), input);
    }

    #[test]
    fn round_trips_boundaries_and_pseudorandom_data() {
        for length in [0, 1, 2, 3, 65, 66, 67, 127, 128, 129, 4095, 4096] {
            let mut state = 0xa178_9d35_u32;
            let mut input = Vec::with_capacity(length);
            for _ in 0..length {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                input.push(u8::try_from(state >> 24).unwrap());
            }
            let encoded = encode(&input).unwrap();
            assert_eq!(decode(&encoded, length).unwrap(), input, "length {length}");
        }
        for seed in 0..16_u32 {
            let length = 257 + usize::try_from(seed).unwrap() * 509;
            let mut state = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15);
            let mut input = Vec::with_capacity(length);
            for _ in 0..length {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                input.push(u8::try_from(state >> 24).unwrap());
            }
            let encoded = encode(&input).unwrap();
            assert_eq!(decode(&encoded, length).unwrap(), input, "seed {seed}");
        }
    }

    #[test]
    fn rejects_invalid_tables_truncation_and_padding() {
        assert_eq!(decode(&[], 0), Err(DecodeError::TruncatedHeader));
        let mut oversubscribed = vec![0u8; HEADER_BYTES];
        oversubscribed[0] = 0x11;
        oversubscribed[1] = 0x01;
        assert_eq!(
            decode(&oversubscribed, 0),
            Err(DecodeError::OversubscribedCodebook)
        );

        let encoded = encode(b"abcabcabcabc").unwrap();
        assert_eq!(
            decode(&encoded[..encoded.len() - 1], 12),
            Err(DecodeError::TruncatedBitstream)
        );
        let mut nonzero_padding = encode(b"abcabcabcabc").unwrap();
        *nonzero_padding.last_mut().unwrap() |= 1;
        assert_eq!(
            decode(&nonzero_padding, 12),
            Err(DecodeError::NonzeroPadding)
        );
    }
}
