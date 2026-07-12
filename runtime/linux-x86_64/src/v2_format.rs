//! Bounded parsing and mapping-plan validation for executable format v2.

use crate::hash;

pub const TRAILER_LEN: usize = 128;
pub const HEADER_LEN: usize = 192;
pub const MANIFEST_HEADER_LEN: usize = 40;
pub const MANIFEST_SEGMENT_LEN: usize = 48;
pub const MAX_SEGMENTS: usize = 128;
pub const MAX_ORIGINAL_SIZE: u64 = 1 << 30;
pub const MAX_PAYLOAD_SIZE: u64 = MAX_ORIGINAL_SIZE + (64 << 20);
pub const MAX_LOADER_SIZE: u64 = 23_500;
pub const PAGE_SIZE: u64 = 4096;
pub const CODEC_LZMA1: u16 = 3;
pub const CODEC_LZMA1_BCJ4: u16 = 4;
pub const CODEC4_CHUNK_COUNT: usize = 4;
pub const CODEC4_TABLE_LEN: usize = 128;

const TRAILER_MAGIC: &[u8; 8] = b"PFGEXE02";
const HEADER_MAGIC: &[u8; 8] = b"PFGIMG02";
const MANIFEST_MAGIC: &[u8; 8] = b"PFGMAN00";
const FIXED_LZMA_PROPERTIES: u8 = 0x5d;
const MIN_DICTIONARY_SIZE: u32 = 1 << 12;
const MAX_DICTIONARY_SIZE: u32 = 1 << 26;
const TRAILER_HASH_OFFSET: usize = 96;
const HEADER_HASH_OFFSET: usize = 160;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Framing,
    Integrity,
    Metadata,
    Range,
    Manifest,
    Elf,
    Overlap,
    Permissions,
    Entry,
    ProgramHeaders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trailer {
    pub image_offset: u64,
    pub image_length: u64,
    pub executable_length: u64,
    pub loader_length: u64,
    pub loader_digest: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub codec: u16,
    pub properties: [u8; 5],
    pub trailing_bytes: u8,
    pub manifest_length: u64,
    pub payload_length: u64,
    pub original_length: u64,
    pub original_digest: [u8; 32],
    pub manifest_digest: [u8; 32],
    pub payload_digest: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Codec4Chunk {
    pub decoded_offset: usize,
    pub decoded_length: usize,
    pub compressed_offset: usize,
    pub compressed_length: usize,
    pub trailing_bytes: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub file_offset: u64,
    pub file_size: u64,
    pub virtual_address: u64,
    pub memory_size: u64,
    pub alignment: u64,
    pub flags: u32,
    pub map_start: u64,
    pub map_length: u64,
}

impl Segment {
    const ZERO: Self = Self {
        file_offset: 0,
        file_size: 0,
        virtual_address: 0,
        memory_size: 0,
        alignment: 0,
        flags: 0,
        map_start: 0,
        map_length: 0,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub original_size: u64,
    pub entry_point: u64,
    pub count: usize,
    pub segments: [Segment; MAX_SEGMENTS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElfInfo {
    pub program_header_address: u64,
    pub program_header_entry_size: u16,
    pub program_header_count: u16,
    pub entry_point: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLayout {
    pub start: u64,
    pub length: u64,
    pub file_start: u64,
}

pub fn parse_trailer(bytes: &[u8; TRAILER_LEN], file_length: u64) -> Result<Trailer, Error> {
    if &bytes[..8] != TRAILER_MAGIC
        || get_u16(bytes, 8) != 2
        || get_u16(bytes, 10) != TRAILER_LEN as u16
    {
        return Err(Error::Framing);
    }
    let mut hash_input = *bytes;
    let stored_hash = array_32(bytes, TRAILER_HASH_OFFSET);
    hash_input[TRAILER_HASH_OFFSET..].fill(0);
    if hash::hash(&hash_input) != stored_hash {
        return Err(Error::Integrity);
    }
    if get_u16(bytes, 12) != 2
        || get_u16(bytes, 14) != 0
        || get_u16(bytes, 80) != 2
        || get_u16(bytes, 82) != 1
        || get_u16(bytes, 84) != 62
        || bytes[86..96].iter().any(|byte| *byte != 0)
    {
        return Err(Error::Metadata);
    }
    let trailer = Trailer {
        image_offset: get_u64(bytes, 16),
        image_length: get_u64(bytes, 24),
        executable_length: get_u64(bytes, 32),
        loader_length: get_u64(bytes, 40),
        loader_digest: array_32(bytes, 48),
    };
    if trailer.executable_length != file_length
        || trailer.loader_length == 0
        || trailer.loader_length > MAX_LOADER_SIZE
        || trailer.image_offset != trailer.loader_length
        || trailer.image_length < HEADER_LEN as u64 + MANIFEST_HEADER_LEN as u64 + 1
        || trailer.image_offset.checked_add(trailer.image_length)
            != file_length.checked_sub(TRAILER_LEN as u64)
    {
        return Err(Error::Range);
    }
    Ok(trailer)
}

pub fn parse_header(bytes: &[u8; HEADER_LEN]) -> Result<Header, Error> {
    if &bytes[..8] != HEADER_MAGIC
        || get_u16(bytes, 8) != 2
        || get_u16(bytes, 10) != HEADER_LEN as u16
    {
        return Err(Error::Framing);
    }
    let mut hash_input = *bytes;
    let stored_hash = array_32(bytes, HEADER_HASH_OFFSET);
    hash_input[HEADER_HASH_OFFSET..].fill(0);
    if hash::hash(&hash_input) != stored_hash {
        return Err(Error::Integrity);
    }
    let properties = [bytes[20], bytes[21], bytes[22], bytes[23], bytes[24]];
    let dictionary =
        u32::from_le_bytes([properties[1], properties[2], properties[3], properties[4]]);
    let codec = get_u16(bytes, 12);
    if !matches!(codec, CODEC_LZMA1 | CODEC_LZMA1_BCJ4)
        || get_u16(bytes, 14) != 0
        || bytes[26..32].iter().any(|byte| *byte != 0)
        || bytes[56..64].iter().any(|byte| *byte != 0)
        || properties[0] != FIXED_LZMA_PROPERTIES
        || !(MIN_DICTIONARY_SIZE..=MAX_DICTIONARY_SIZE).contains(&dictionary)
        || (codec == CODEC_LZMA1 && bytes[25] > 5)
        || (codec == CODEC_LZMA1_BCJ4 && bytes[25] != 0)
    {
        return Err(Error::Metadata);
    }
    let header = Header {
        codec,
        properties,
        trailing_bytes: bytes[25],
        manifest_length: get_u64(bytes, 32),
        payload_length: get_u64(bytes, 40),
        original_length: get_u64(bytes, 48),
        original_digest: array_32(bytes, 64),
        manifest_digest: array_32(bytes, 96),
        payload_digest: array_32(bytes, 128),
    };
    if header.manifest_length < MANIFEST_HEADER_LEN as u64
        || header.manifest_length
            > (MANIFEST_HEADER_LEN + MAX_SEGMENTS * MANIFEST_SEGMENT_LEN) as u64
        || header.payload_length == 0
        || header.payload_length > MAX_PAYLOAD_SIZE
        || header.original_length == 0
        || header.original_length > MAX_ORIGINAL_SIZE
    {
        return Err(Error::Range);
    }
    Ok(header)
}

pub fn parse_codec4_chunks(
    payload: &[u8],
    original_length: usize,
) -> Result<[Codec4Chunk; CODEC4_CHUNK_COUNT], Error> {
    const ENTRY_LEN: usize = CODEC4_TABLE_LEN / CODEC4_CHUNK_COUNT;
    const TRAILING_SHIFT: u32 = 56;
    const LENGTH_MASK: u64 = (1u64 << TRAILING_SHIFT) - 1;
    if payload.len() <= CODEC4_TABLE_LEN {
        return Err(Error::Range);
    }
    let maximum_chunk = original_length
        .checked_mul(28)
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .ok_or(Error::Range)?;
    let mut expected_decoded = 0usize;
    let mut expected_compressed = CODEC4_TABLE_LEN;
    let mut chunks = [Codec4Chunk {
        decoded_offset: 0,
        decoded_length: 0,
        compressed_offset: 0,
        compressed_length: 0,
        trailing_bytes: 0,
    }; CODEC4_CHUNK_COUNT];
    for (index, chunk) in chunks.iter_mut().enumerate() {
        let offset = index * ENTRY_LEN;
        let encoded_length = get_u64(payload, offset + 8);
        *chunk = Codec4Chunk {
            decoded_offset: to_usize(get_u64(payload, offset))?,
            decoded_length: to_usize(encoded_length & LENGTH_MASK)?,
            compressed_offset: to_usize(get_u64(payload, offset + 16))?,
            compressed_length: to_usize(get_u64(payload, offset + 24))?,
            trailing_bytes: (encoded_length >> TRAILING_SHIFT) as u8,
        };
        if chunk.decoded_offset != expected_decoded
            || chunk.compressed_offset != expected_compressed
            || chunk.decoded_length == 0
            || chunk.decoded_length > maximum_chunk
            || chunk.compressed_length == 0
            || chunk.trailing_bytes > 5
        {
            return Err(Error::Range);
        }
        expected_decoded = expected_decoded
            .checked_add(chunk.decoded_length)
            .ok_or(Error::Range)?;
        expected_compressed = expected_compressed
            .checked_add(chunk.compressed_length)
            .ok_or(Error::Range)?;
    }
    if expected_decoded != original_length || expected_compressed != payload.len() {
        return Err(Error::Range);
    }
    Ok(chunks)
}

pub fn validate_image_layout(trailer: &Trailer, header: &Header) -> Result<(), Error> {
    let expected = (HEADER_LEN as u64)
        .checked_add(header.manifest_length)
        .and_then(|length| length.checked_add(header.payload_length))
        .ok_or(Error::Range)?;
    if expected != trailer.image_length {
        return Err(Error::Range);
    }
    Ok(())
}

pub fn parse_manifest(input: &[u8], expected_original_size: u64) -> Result<Manifest, Error> {
    if input.len() < MANIFEST_HEADER_LEN
        || input.get(..8) != Some(MANIFEST_MAGIC)
        || get_u16(input, 8) != 0
        || get_u16(input, 10) != MANIFEST_HEADER_LEN as u16
        || get_u16(input, 12) != MANIFEST_SEGMENT_LEN as u16
        || input[16..24].iter().any(|byte| *byte != 0)
    {
        return Err(Error::Manifest);
    }
    let count = usize::from(get_u16(input, 14));
    let expected_length = count
        .checked_mul(MANIFEST_SEGMENT_LEN)
        .and_then(|length| length.checked_add(MANIFEST_HEADER_LEN))
        .ok_or(Error::Range)?;
    if count == 0 || count > MAX_SEGMENTS || input.len() != expected_length {
        return Err(Error::Manifest);
    }
    let original_size = get_u64(input, 24);
    let entry_point = get_u64(input, 32);
    if original_size != expected_original_size {
        return Err(Error::Manifest);
    }
    let mut segments = [Segment::ZERO; MAX_SEGMENTS];
    let mut total_memory = 0u64;
    let mut entry_is_executable = false;
    for (index, segment) in segments.iter_mut().take(count).enumerate() {
        let offset = MANIFEST_HEADER_LEN + index * MANIFEST_SEGMENT_LEN;
        if get_u32(input, offset + 44) != 0 {
            return Err(Error::Manifest);
        }
        let file_offset = get_u64(input, offset);
        let file_size = get_u64(input, offset + 8);
        let virtual_address = get_u64(input, offset + 16);
        let memory_size = get_u64(input, offset + 24);
        let alignment = get_u64(input, offset + 32);
        let flags = get_u32(input, offset + 40);
        let file_end = file_offset.checked_add(file_size).ok_or(Error::Range)?;
        let memory_end = virtual_address
            .checked_add(memory_size)
            .ok_or(Error::Range)?;
        if memory_size == 0
            || file_size > memory_size
            || file_end > original_size
            || flags & !7 != 0
        {
            return Err(Error::Manifest);
        }
        if flags & 3 == 3 {
            return Err(Error::Permissions);
        }
        if alignment > 1
            && (!alignment.is_power_of_two()
                || file_offset % alignment != virtual_address % alignment)
        {
            return Err(Error::Manifest);
        }
        let map_start = virtual_address & !(PAGE_SIZE - 1);
        let map_end = align_up(memory_end, PAGE_SIZE)?;
        if map_start < PAGE_SIZE || map_end <= map_start {
            return Err(Error::Range);
        }
        *segment = Segment {
            file_offset,
            file_size,
            virtual_address,
            memory_size,
            alignment,
            flags,
            map_start,
            map_length: map_end - map_start,
        };
        total_memory = total_memory.checked_add(memory_size).ok_or(Error::Range)?;
        if flags & 1 != 0 && entry_point >= virtual_address && entry_point < memory_end {
            entry_is_executable = true;
        }
    }
    if total_memory > MAX_ORIGINAL_SIZE || !entry_is_executable {
        return Err(if entry_is_executable {
            Error::Range
        } else {
            Error::Entry
        });
    }
    for left in 0..count {
        let left_end = segments[left]
            .map_start
            .checked_add(segments[left].map_length)
            .ok_or(Error::Range)?;
        for right in left + 1..count {
            let right_end = segments[right]
                .map_start
                .checked_add(segments[right].map_length)
                .ok_or(Error::Range)?;
            if segments[left].map_start < right_end && segments[right].map_start < left_end {
                return Err(Error::Overlap);
            }
        }
    }
    Ok(Manifest {
        original_size,
        entry_point,
        count,
        segments,
    })
}

pub fn direct_output_layout(manifest: &Manifest) -> Result<OutputLayout, Error> {
    let first = manifest.segments.first().ok_or(Error::Manifest)?;
    let file_start = first
        .virtual_address
        .checked_sub(first.file_offset)
        .ok_or(Error::Range)?;
    if file_start < PAGE_SIZE || file_start & (PAGE_SIZE - 1) != 0 {
        return Err(Error::Manifest);
    }
    let mut start = file_start;
    let mut end = file_start
        .checked_add(manifest.original_size)
        .ok_or(Error::Range)?;
    let mut previous_map_end = 0u64;
    let mut previous_source_end = 0u64;
    let mut forward_destination_end = 0u64;
    for segment in manifest.segments.iter().take(manifest.count) {
        let source_start = file_start
            .checked_add(segment.file_offset)
            .ok_or(Error::Range)?;
        let source_end = source_start
            .checked_add(segment.file_size)
            .ok_or(Error::Range)?;
        let map_end = segment
            .map_start
            .checked_add(segment.map_length)
            .ok_or(Error::Range)?;
        let destination_end = segment
            .virtual_address
            .checked_add(segment.file_size)
            .ok_or(Error::Range)?;
        if segment.map_start < previous_map_end
            || source_start < previous_source_end
            || forward_destination_end > source_start
            || (segment.virtual_address < source_start
                && segment.virtual_address < previous_source_end)
        {
            return Err(Error::Overlap);
        }
        forward_destination_end = if segment.virtual_address > source_start {
            destination_end
        } else {
            0
        };
        previous_map_end = map_end;
        previous_source_end = source_end;
        start = start.min(segment.map_start);
        end = end.max(
            segment
                .virtual_address
                .checked_add(segment.memory_size)
                .ok_or(Error::Range)?,
        );
    }
    end = align_up(end, PAGE_SIZE)?;
    let length = end.checked_sub(start).ok_or(Error::Range)?;
    if length == 0 || length > MAX_ORIGINAL_SIZE {
        return Err(Error::Range);
    }
    Ok(OutputLayout {
        start,
        length,
        file_start,
    })
}

pub fn validate_elf(original: &[u8], manifest: &Manifest) -> Result<ElfInfo, Error> {
    let header = original.get(..64).ok_or(Error::Elf)?;
    if header.get(..7) != Some(b"\x7fELF\x02\x01\x01")
        || get_u16(header, 16) != 2
        || get_u16(header, 18) != 62
        || get_u32(header, 20) != 1
        || get_u64(header, 24) != manifest.entry_point
        || get_u16(header, 52) != 64
        || get_u16(header, 54) != 56
    {
        return Err(Error::Elf);
    }
    let phoff = get_u64(header, 32);
    let phnum = get_u16(header, 56);
    let table_length = u64::from(phnum).checked_mul(56).ok_or(Error::Range)?;
    let table_end = phoff.checked_add(table_length).ok_or(Error::Range)?;
    if phnum == 0 || table_end > original.len() as u64 {
        return Err(Error::ProgramHeaders);
    }
    let mut load_index = 0usize;
    for index in 0..phnum {
        let offset = usize::try_from(phoff + u64::from(index) * 56).map_err(|_| Error::Range)?;
        let program = original
            .get(offset..offset + 56)
            .ok_or(Error::ProgramHeaders)?;
        match get_u32(program, 0) {
            PT_INTERP | PT_DYNAMIC => return Err(Error::Elf),
            PT_LOAD => {
                let expected = manifest
                    .segments
                    .get(load_index)
                    .filter(|_| load_index < manifest.count)
                    .ok_or(Error::Manifest)?;
                if get_u32(program, 4) != expected.flags
                    || get_u64(program, 8) != expected.file_offset
                    || get_u64(program, 16) != expected.virtual_address
                    || get_u64(program, 32) != expected.file_size
                    || get_u64(program, 40) != expected.memory_size
                    || get_u64(program, 48) != expected.alignment
                {
                    return Err(Error::Manifest);
                }
                load_index += 1;
            }
            _ => {}
        }
    }
    if load_index != manifest.count {
        return Err(Error::Manifest);
    }
    let program_header_address = file_offset_to_address(phoff, table_length, manifest)?;
    Ok(ElfInfo {
        program_header_address,
        program_header_entry_size: 56,
        program_header_count: phnum,
        entry_point: manifest.entry_point,
    })
}

fn file_offset_to_address(offset: u64, length: u64, manifest: &Manifest) -> Result<u64, Error> {
    let end = offset.checked_add(length).ok_or(Error::Range)?;
    for segment in manifest.segments.iter().take(manifest.count) {
        let file_end = segment
            .file_offset
            .checked_add(segment.file_size)
            .ok_or(Error::Range)?;
        if offset >= segment.file_offset && end <= file_end {
            return segment
                .virtual_address
                .checked_add(offset - segment.file_offset)
                .ok_or(Error::Range);
        }
    }
    Err(Error::ProgramHeaders)
}

fn align_up(value: u64, alignment: u64) -> Result<u64, Error> {
    value
        .checked_add(alignment - 1)
        .map(|rounded| rounded & !(alignment - 1))
        .ok_or(Error::Range)
}

fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([input[offset], input[offset + 1]])
}

fn get_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
        input[offset + 4],
        input[offset + 5],
        input[offset + 6],
        input[offset + 7],
    ])
}

fn to_usize(value: u64) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| Error::Range)
}

fn array_32(input: &[u8], offset: usize) -> [u8; 32] {
    let mut output = [0u8; 32];
    output.copy_from_slice(&input[offset..offset + 32]);
    output
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::{
        Error, HEADER_LEN, MANIFEST_HEADER_LEN, MANIFEST_SEGMENT_LEN, direct_output_layout,
        parse_header, parse_manifest,
    };

    #[test]
    fn rejects_unhashed_header_and_writable_executable_manifest() {
        assert_eq!(parse_header(&[0u8; HEADER_LEN]), Err(Error::Framing));

        let mut manifest = vec![0u8; MANIFEST_HEADER_LEN + MANIFEST_SEGMENT_LEN];
        manifest[..8].copy_from_slice(b"PFGMAN00");
        manifest[10..12].copy_from_slice(&(MANIFEST_HEADER_LEN as u16).to_le_bytes());
        manifest[12..14].copy_from_slice(&(MANIFEST_SEGMENT_LEN as u16).to_le_bytes());
        manifest[14..16].copy_from_slice(&1u16.to_le_bytes());
        manifest[24..32].copy_from_slice(&4096u64.to_le_bytes());
        manifest[32..40].copy_from_slice(&0x400100u64.to_le_bytes());
        let offset = MANIFEST_HEADER_LEN;
        manifest[offset + 8..offset + 16].copy_from_slice(&4096u64.to_le_bytes());
        manifest[offset + 16..offset + 24].copy_from_slice(&0x401000u64.to_le_bytes());
        manifest[offset + 24..offset + 32].copy_from_slice(&4096u64.to_le_bytes());
        manifest[offset + 32..offset + 40].copy_from_slice(&4096u64.to_le_bytes());
        manifest[offset + 40..offset + 44].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(parse_manifest(&manifest, 4096), Err(Error::Permissions));
    }

    #[test]
    fn requires_one_bounded_direct_output_bias() {
        let mut manifest = vec![0u8; MANIFEST_HEADER_LEN + 2 * MANIFEST_SEGMENT_LEN];
        manifest[..8].copy_from_slice(b"PFGMAN00");
        manifest[10..12].copy_from_slice(&(MANIFEST_HEADER_LEN as u16).to_le_bytes());
        manifest[12..14].copy_from_slice(&(MANIFEST_SEGMENT_LEN as u16).to_le_bytes());
        manifest[14..16].copy_from_slice(&2u16.to_le_bytes());
        manifest[24..32].copy_from_slice(&0x3000u64.to_le_bytes());
        manifest[32..40].copy_from_slice(&0x400100u64.to_le_bytes());
        for (index, (file, address, flags)) in [(0u64, 0x400000u64, 5u32), (0x2000, 0x402000, 4)]
            .into_iter()
            .enumerate()
        {
            let offset = MANIFEST_HEADER_LEN + index * MANIFEST_SEGMENT_LEN;
            manifest[offset..offset + 8].copy_from_slice(&file.to_le_bytes());
            manifest[offset + 8..offset + 16].copy_from_slice(&0x1000u64.to_le_bytes());
            manifest[offset + 16..offset + 24].copy_from_slice(&address.to_le_bytes());
            manifest[offset + 24..offset + 32].copy_from_slice(&0x1000u64.to_le_bytes());
            manifest[offset + 32..offset + 40].copy_from_slice(&0x1000u64.to_le_bytes());
            manifest[offset + 40..offset + 44].copy_from_slice(&flags.to_le_bytes());
        }
        let parsed = parse_manifest(&manifest, 0x3000).unwrap();
        assert_eq!(direct_output_layout(&parsed).unwrap().file_start, 0x400000);

        manifest[MANIFEST_HEADER_LEN + MANIFEST_SEGMENT_LEN + 16
            ..MANIFEST_HEADER_LEN + MANIFEST_SEGMENT_LEN + 24]
            .copy_from_slice(&0x403000u64.to_le_bytes());
        let parsed = parse_manifest(&manifest, 0x3000).unwrap();
        assert_eq!(direct_output_layout(&parsed).unwrap().file_start, 0x400000);
    }
}
