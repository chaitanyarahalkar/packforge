//! Bounded binary segment manifest used by future format-aware runtimes.

use std::fmt;

use serde::Serialize;

use crate::MAX_ORIGINAL_SIZE;
use crate::format::{FormatError, classify_with_load_segments};

/// Manifest format version established by M0.
pub const MANIFEST_VERSION: u16 = 0;
/// Fixed manifest header size.
pub const MANIFEST_HEADER_LEN: usize = 40;
const MANIFEST_HEADER_LEN_U16: u16 = 40;
/// Fixed segment record size.
pub const MANIFEST_SEGMENT_LEN: usize = 48;
const MANIFEST_SEGMENT_LEN_U16: u16 = 48;
/// Maximum number of load-segment descriptions in one manifest.
pub const MAX_MANIFEST_SEGMENTS: usize = 128;
/// Maximum sum of segment memory sizes represented by manifest v0.
pub const MAX_MANIFEST_MEMORY_SIZE: u64 = MAX_ORIGINAL_SIZE;

const MAGIC: &[u8; 8] = b"PFGMAN00";
const KNOWN_FLAGS: u32 = 7;

/// A complete, deterministic manifest v0 value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestV0 {
    /// Exact original executable size.
    pub original_size: u64,
    /// Original executable entry point.
    pub entry_point: u64,
    /// Ordered load-segment descriptions.
    pub segments: Vec<ManifestSegment>,
}

/// One bounded load-segment description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ManifestSegment {
    /// Offset of initialized bytes in the original file.
    pub file_offset: u64,
    /// Number of initialized bytes copied from the original file.
    pub file_size: u64,
    /// Target virtual address.
    pub virtual_address: u64,
    /// Complete in-memory size, including zero-filled bytes.
    pub memory_size: u64,
    /// ELF-compatible alignment: zero, one, or a power of two.
    pub alignment: u64,
    /// ELF-compatible read/write/execute bits in the low three bits.
    pub flags: u32,
}

/// Errors returned by manifest validation and decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Input is shorter than the fixed header or declared records.
    Truncated,
    /// The manifest magic is not recognized.
    InvalidMagic,
    /// The manifest version is unsupported.
    UnsupportedVersion(u16),
    /// The fixed header or record length is unsupported.
    InvalidLayout,
    /// Header or record reserved fields are nonzero.
    NonzeroReserved,
    /// Segment count is zero or exceeds the hard limit.
    InvalidSegmentCount(usize),
    /// Encoded input has trailing bytes or an inconsistent length.
    InvalidEncodedLength,
    /// Original executable length is zero or exceeds the hard limit.
    InvalidOriginalSize(u64),
    /// A segment has an invalid file or memory range.
    InvalidSegmentRange(usize),
    /// A segment alignment is invalid or incongruent with its address/offset.
    InvalidSegmentAlignment(usize),
    /// A segment uses flags outside the read/write/execute mask.
    InvalidSegmentFlags(usize),
    /// The total described memory exceeds the hard limit.
    MemoryLimitExceeded(u64),
    /// A checked size calculation overflowed.
    SizeOverflow,
}

/// Errors produced while deriving a canonical manifest from an ELF executable.
#[derive(Debug)]
pub enum ManifestElfError {
    /// The source executable is outside the supported ELF compatibility tier.
    Format(FormatError),
    /// The derived segment description violates manifest v0 bounds.
    Manifest(ManifestError),
}

impl fmt::Display for ManifestElfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(formatter),
            Self::Manifest(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ManifestElfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::Manifest(error) => Some(error),
        }
    }
}

impl From<FormatError> for ManifestElfError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}

impl From<ManifestError> for ManifestElfError {
    fn from(error: ManifestError) -> Self {
        Self::Manifest(error)
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => formatter.write_str("truncated Packforge manifest"),
            Self::InvalidMagic => formatter.write_str("input is not a Packforge manifest"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported Packforge manifest version {version}"
                )
            }
            Self::InvalidLayout => formatter.write_str("unsupported Packforge manifest layout"),
            Self::NonzeroReserved => {
                formatter.write_str("manifest uses unsupported flags or reserved fields")
            }
            Self::InvalidSegmentCount(count) => {
                write!(
                    formatter,
                    "manifest segment count {count} is outside the supported range"
                )
            }
            Self::InvalidEncodedLength => {
                formatter.write_str("manifest encoded length is inconsistent")
            }
            Self::InvalidOriginalSize(size) => {
                write!(
                    formatter,
                    "manifest original size {size} is outside the supported range"
                )
            }
            Self::InvalidSegmentRange(index) => {
                write!(formatter, "manifest segment {index} has an invalid range")
            }
            Self::InvalidSegmentAlignment(index) => {
                write!(formatter, "manifest segment {index} has invalid alignment")
            }
            Self::InvalidSegmentFlags(index) => {
                write!(formatter, "manifest segment {index} has unsupported flags")
            }
            Self::MemoryLimitExceeded(size) => {
                write!(
                    formatter,
                    "manifest describes {size} bytes of memory, exceeding the limit"
                )
            }
            Self::SizeOverflow => formatter.write_str("manifest size calculation overflowed"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl ManifestV0 {
    /// Validates and encodes this manifest deterministically.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] if any count, range, alignment, flag, or total
    /// memory bound is invalid.
    pub fn encode(&self) -> Result<Vec<u8>, ManifestError> {
        self.validate()?;
        let length = encoded_length(self.segments.len())?;
        let mut output = vec![0u8; length];
        output[..8].copy_from_slice(MAGIC);
        put_u16(&mut output, 8, MANIFEST_VERSION);
        put_u16(&mut output, 10, MANIFEST_HEADER_LEN_U16);
        put_u16(&mut output, 12, MANIFEST_SEGMENT_LEN_U16);
        put_u16(
            &mut output,
            14,
            u16::try_from(self.segments.len()).map_err(|_| ManifestError::SizeOverflow)?,
        );
        put_u64(&mut output, 24, self.original_size);
        put_u64(&mut output, 32, self.entry_point);
        for (index, segment) in self.segments.iter().enumerate() {
            let offset = MANIFEST_HEADER_LEN + index * MANIFEST_SEGMENT_LEN;
            put_u64(&mut output, offset, segment.file_offset);
            put_u64(&mut output, offset + 8, segment.file_size);
            put_u64(&mut output, offset + 16, segment.virtual_address);
            put_u64(&mut output, offset + 24, segment.memory_size);
            put_u64(&mut output, offset + 32, segment.alignment);
            put_u32(&mut output, offset + 40, segment.flags);
        }
        Ok(output)
    }

    /// Validates all manifest invariants without encoding.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the manifest is outside v0 bounds.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.original_size == 0 || self.original_size > MAX_ORIGINAL_SIZE {
            return Err(ManifestError::InvalidOriginalSize(self.original_size));
        }
        if self.segments.is_empty() || self.segments.len() > MAX_MANIFEST_SEGMENTS {
            return Err(ManifestError::InvalidSegmentCount(self.segments.len()));
        }
        let mut total_memory = 0u64;
        for (index, segment) in self.segments.iter().enumerate() {
            validate_segment(index, segment, self.original_size)?;
            total_memory = total_memory
                .checked_add(segment.memory_size)
                .ok_or(ManifestError::SizeOverflow)?;
        }
        if total_memory > MAX_MANIFEST_MEMORY_SIZE {
            return Err(ManifestError::MemoryLimitExceeded(total_memory));
        }
        Ok(())
    }
}

/// Derives manifest v0 from every validated `PT_LOAD` record in source order.
///
/// # Errors
///
/// Returns [`ManifestElfError`] when the source is outside the supported static
/// ELF64 x86-64 tier or its load description exceeds manifest v0 bounds.
pub fn manifest_from_elf(input: &[u8]) -> Result<ManifestV0, ManifestElfError> {
    let (binary, load_segments) = classify_with_load_segments(input)?;
    let manifest = ManifestV0 {
        original_size: u64::try_from(input.len()).map_err(|_| ManifestError::SizeOverflow)?,
        entry_point: binary.entry_point,
        segments: load_segments
            .into_iter()
            .map(|segment| ManifestSegment {
                file_offset: segment.file_offset,
                file_size: segment.file_size,
                virtual_address: segment.virtual_address,
                memory_size: segment.memory_size,
                alignment: segment.alignment,
                flags: segment.flags,
            })
            .collect(),
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Decodes and validates an exact manifest v0 byte sequence.
///
/// # Errors
///
/// Returns [`ManifestError`] for malformed framing, unknown metadata, unsafe
/// ranges, invalid alignment/flags, or hard-limit violations.
pub fn decode_manifest_v0(input: &[u8]) -> Result<ManifestV0, ManifestError> {
    if input.len() < MANIFEST_HEADER_LEN {
        return Err(ManifestError::Truncated);
    }
    if input.get(..8) != Some(MAGIC) {
        return Err(ManifestError::InvalidMagic);
    }
    let version = get_u16(input, 8)?;
    if version != MANIFEST_VERSION {
        return Err(ManifestError::UnsupportedVersion(version));
    }
    if usize::from(get_u16(input, 10)?) != MANIFEST_HEADER_LEN
        || usize::from(get_u16(input, 12)?) != MANIFEST_SEGMENT_LEN
    {
        return Err(ManifestError::InvalidLayout);
    }
    if input[16..24].iter().any(|byte| *byte != 0) {
        return Err(ManifestError::NonzeroReserved);
    }
    let segment_count = usize::from(get_u16(input, 14)?);
    if segment_count == 0 || segment_count > MAX_MANIFEST_SEGMENTS {
        return Err(ManifestError::InvalidSegmentCount(segment_count));
    }
    let expected_length = encoded_length(segment_count)?;
    if input.len() != expected_length {
        return Err(ManifestError::InvalidEncodedLength);
    }
    let original_size = get_u64(input, 24)?;
    let entry_point = get_u64(input, 32)?;
    let mut segments = Vec::with_capacity(segment_count);
    for index in 0..segment_count {
        let offset = MANIFEST_HEADER_LEN + index * MANIFEST_SEGMENT_LEN;
        if get_u32(input, offset + 44)? != 0 {
            return Err(ManifestError::NonzeroReserved);
        }
        segments.push(ManifestSegment {
            file_offset: get_u64(input, offset)?,
            file_size: get_u64(input, offset + 8)?,
            virtual_address: get_u64(input, offset + 16)?,
            memory_size: get_u64(input, offset + 24)?,
            alignment: get_u64(input, offset + 32)?,
            flags: get_u32(input, offset + 40)?,
        });
    }
    let manifest = ManifestV0 {
        original_size,
        entry_point,
        segments,
    };
    manifest.validate()?;
    Ok(manifest)
}

fn validate_segment(
    index: usize,
    segment: &ManifestSegment,
    original_size: u64,
) -> Result<(), ManifestError> {
    let file_end = segment
        .file_offset
        .checked_add(segment.file_size)
        .ok_or(ManifestError::InvalidSegmentRange(index))?;
    if segment.memory_size == 0
        || segment.file_size > segment.memory_size
        || file_end > original_size
        || segment
            .virtual_address
            .checked_add(segment.memory_size)
            .is_none()
    {
        return Err(ManifestError::InvalidSegmentRange(index));
    }
    if segment.alignment > 1
        && (!segment.alignment.is_power_of_two()
            || segment.file_offset % segment.alignment
                != segment.virtual_address % segment.alignment)
    {
        return Err(ManifestError::InvalidSegmentAlignment(index));
    }
    if segment.flags & !KNOWN_FLAGS != 0 {
        return Err(ManifestError::InvalidSegmentFlags(index));
    }
    Ok(())
}

fn encoded_length(segment_count: usize) -> Result<usize, ManifestError> {
    segment_count
        .checked_mul(MANIFEST_SEGMENT_LEN)
        .and_then(|length| length.checked_add(MANIFEST_HEADER_LEN))
        .ok_or(ManifestError::SizeOverflow)
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(input: &[u8], offset: usize) -> Result<u16, ManifestError> {
    let bytes = input
        .get(offset..offset + 2)
        .ok_or(ManifestError::Truncated)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn get_u32(input: &[u8], offset: usize) -> Result<u32, ManifestError> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or(ManifestError::Truncated)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn get_u64(input: &[u8], offset: usize) -> Result<u64, ManifestError> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or(ManifestError::Truncated)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::{
        MANIFEST_HEADER_LEN, MANIFEST_SEGMENT_LEN, ManifestError, ManifestSegment, ManifestV0,
        decode_manifest_v0, manifest_from_elf,
    };

    fn elf_fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 4_096];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x40_1000u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x40_0000u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&4_096u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&4_096u64.to_le_bytes());
        bytes[112..120].copy_from_slice(&4_096u64.to_le_bytes());
        bytes
    }

    fn manifest() -> ManifestV0 {
        ManifestV0 {
            original_size: 0x5000,
            entry_point: 0x0040_1000,
            segments: vec![
                ManifestSegment {
                    file_offset: 0,
                    file_size: 0x2000,
                    virtual_address: 0x0040_0000,
                    memory_size: 0x2000,
                    alignment: 0x1000,
                    flags: 5,
                },
                ManifestSegment {
                    file_offset: 0x2000,
                    file_size: 0x1000,
                    virtual_address: 0x0040_2000,
                    memory_size: 0x1800,
                    alignment: 0x1000,
                    flags: 6,
                },
            ],
        }
    }

    #[test]
    fn round_trips_deterministically() {
        let manifest = manifest();
        let first = manifest.encode().unwrap();
        let second = manifest.encode().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), MANIFEST_HEADER_LEN + 2 * MANIFEST_SEGMENT_LEN);
        assert_eq!(decode_manifest_v0(&first).unwrap(), manifest);
    }

    #[test]
    fn derives_canonical_manifest_from_elf_program_headers() {
        let input = elf_fixture();
        let manifest = manifest_from_elf(&input).unwrap();
        assert_eq!(manifest.original_size, 4_096);
        assert_eq!(manifest.entry_point, 0x40_1000);
        assert_eq!(manifest.segments.len(), 1);
        assert_eq!(manifest.segments[0].virtual_address, 0x40_0000);
        assert_eq!(manifest.segments[0].flags, 5);
        assert_eq!(
            decode_manifest_v0(&manifest.encode().unwrap()).unwrap(),
            manifest
        );
    }

    #[test]
    fn round_trips_many_arbitrary_bounded_descriptions() {
        for count in [1usize, 2, 7, 32, 128] {
            let segments = (0..count)
                .map(|index| ManifestSegment {
                    file_offset: u64::try_from(index).unwrap() * 0x1000,
                    file_size: 0x800,
                    virtual_address: 0x0040_0000 + u64::try_from(index).unwrap() * 0x1000,
                    memory_size: 0x1000,
                    alignment: 0x1000,
                    flags: u32::try_from(index).unwrap() & 7,
                })
                .collect();
            let manifest = ManifestV0 {
                original_size: u64::try_from(count).unwrap() * 0x1000,
                entry_point: 0x0040_0000,
                segments,
            };
            let bytes = manifest.encode().unwrap();
            assert_eq!(decode_manifest_v0(&bytes).unwrap(), manifest);
        }
    }

    #[test]
    fn rejects_truncation_reserved_bytes_and_trailing_data() {
        let bytes = manifest().encode().unwrap();
        assert!(matches!(
            decode_manifest_v0(&bytes[..MANIFEST_HEADER_LEN - 1]),
            Err(ManifestError::Truncated)
        ));
        let mut reserved = bytes.clone();
        reserved[16] = 1;
        assert!(matches!(
            decode_manifest_v0(&reserved),
            Err(ManifestError::NonzeroReserved)
        ));
        let mut trailing = bytes;
        trailing.push(0);
        assert!(matches!(
            decode_manifest_v0(&trailing),
            Err(ManifestError::InvalidEncodedLength)
        ));
    }

    #[test]
    fn rejects_unsafe_ranges_alignment_and_flags() {
        let mut invalid = manifest();
        invalid.segments[0].file_size = invalid.original_size + 1;
        assert!(matches!(
            invalid.encode(),
            Err(ManifestError::InvalidSegmentRange(0))
        ));
        let mut invalid = manifest();
        invalid.segments[0].alignment = 3;
        assert!(matches!(
            invalid.encode(),
            Err(ManifestError::InvalidSegmentAlignment(0))
        ));
        let mut invalid = manifest();
        invalid.segments[0].flags = 8;
        assert!(matches!(
            invalid.encode(),
            Err(ManifestError::InvalidSegmentFlags(0))
        ));
    }
}
