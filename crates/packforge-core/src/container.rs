//! Versioned, deterministic Packforge container encoding.

use std::fmt;
use std::io::{Cursor, Read};

use serde::Serialize;

use crate::format::{
    Architecture, BinaryClass, BinaryFormat, BinaryInfo, BinaryType, Endianness, FormatError,
    classify,
};

/// Current Packforge container version.
pub const CONTAINER_VERSION: u16 = 1;
/// Fixed header length for container version 1.
pub const HEADER_LEN: usize = 192;
const HEADER_LEN_U16: u16 = 192;
const HEADER_LEN_U64: u64 = 192;
/// Maximum original image accepted by the in-memory M1 implementation.
pub const MAX_ORIGINAL_SIZE: u64 = 1 << 30;
/// Maximum compressed payload accepted by the in-memory M1 implementation.
pub const MAX_PAYLOAD_SIZE: u64 = MAX_ORIGINAL_SIZE + (64 << 20);
/// Maximum complete container accepted by the in-memory M1 implementation.
pub const MAX_CONTAINER_SIZE: u64 = MAX_PAYLOAD_SIZE + HEADER_LEN_U64;

const MAGIC: &[u8; 8] = b"PFGCNT01";
const HEADER_HASH_OFFSET: usize = 152;
const HEADER_HASH_END: usize = 184;
const RESERVED_OFFSET: usize = 184;
const FORMAT_ELF: u8 = 1;
const CLASS_ELF64: u8 = 2;
const ENDIAN_LITTLE: u8 = 1;
const MACHINE_X86_64: u16 = 62;
const FILE_TYPE_EXECUTABLE: u16 = 2;
const ZSTD_MAX_WINDOW_LOG: u32 = 27;

/// User-facing compression policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Optimize for encoder and decoder speed using LZ4 block format.
    Fast,
    /// Use a moderate Zstandard level.
    Balanced,
    /// Spend more packing time for a smaller Zstandard payload.
    Small,
    /// Evaluate all stable candidates and choose the smallest payload.
    Auto,
}

impl Profile {
    /// Stable text representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::Small => "small",
            Self::Auto => "auto",
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::Fast => 1,
            Self::Balanced => 2,
            Self::Small => 3,
            Self::Auto => 4,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ContainerError> {
        match tag {
            1 => Ok(Self::Fast),
            2 => Ok(Self::Balanced),
            3 => Ok(Self::Small),
            4 => Ok(Self::Auto),
            _ => Err(ContainerError::UnknownProfile(tag)),
        }
    }
}

/// Codec used by the selected profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    /// LZ4 block format without a size prefix.
    Lz4,
    /// Zstandard frame format.
    Zstd,
}

impl Codec {
    /// Stable text representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lz4 => "lz4",
            Self::Zstd => "zstd",
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::Lz4 => 1,
            Self::Zstd => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ContainerError> {
        match tag {
            1 => Ok(Self::Lz4),
            2 => Ok(Self::Zstd),
            _ => Err(ContainerError::UnknownCodec(tag)),
        }
    }
}

/// How much of an artifact has been validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// Header and compressed payload integrity are valid.
    Payload,
    /// Decompressed size, digest, and executable metadata are also valid.
    Full,
}

/// Options used to produce a deterministic container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackOptions {
    /// Compression policy.
    pub profile: Profile,
    /// Permit an output that is not smaller than the input.
    pub allow_larger: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            profile: Profile::Balanced,
            allow_larger: false,
        }
    }
}

/// Stable information returned by inspect and verify operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactInfo {
    /// JSON/report schema version.
    pub schema_version: u16,
    /// Binary container format version.
    pub container_version: u16,
    /// Validation depth reached by the operation.
    pub verification: Verification,
    /// Requested compression policy.
    pub profile: Profile,
    /// Selected codec.
    pub codec: Codec,
    /// Selected codec level; zero for codecs without levels.
    pub codec_level: i32,
    /// Original file length in bytes.
    pub original_size: u64,
    /// Compressed payload length in bytes.
    pub payload_size: u64,
    /// Header plus payload length in bytes.
    pub container_size: u64,
    /// Payload/original ratio in basis points, where 10,000 is 100%.
    pub payload_ratio_basis_points: u32,
    /// Original executable BLAKE3 digest.
    pub original_digest: String,
    /// Compressed payload BLAKE3 digest.
    pub payload_digest: String,
    /// Original Unix mode when available, otherwise zero.
    pub original_mode: u32,
    /// Executable-format classification.
    pub binary: BinaryInfo,
}

/// Bytes and metadata produced by packing.
#[derive(Debug)]
pub struct PackedArtifact {
    /// Complete container bytes.
    pub bytes: Vec<u8>,
    /// Container metadata.
    pub info: ArtifactInfo,
}

/// Bytes and metadata produced by unpacking.
#[derive(Debug)]
pub struct UnpackedArtifact {
    /// Byte-identical original executable.
    pub bytes: Vec<u8>,
    /// Fully verified container metadata.
    pub info: ArtifactInfo,
}

/// Errors produced by bounded container processing.
#[derive(Debug)]
pub enum ContainerError {
    /// The original executable is outside the M1 compatibility tier.
    Format(FormatError),
    /// An input or declared length exceeds a hard limit.
    SizeLimit {
        /// Length field being validated.
        field: &'static str,
        /// Observed byte length.
        actual: u64,
        /// Maximum permitted byte length.
        maximum: u64,
    },
    /// A container is shorter than its fixed header.
    TruncatedHeader,
    /// The container magic bytes are not recognized.
    InvalidMagic,
    /// The container version is not supported.
    UnsupportedVersion(u16),
    /// The fixed header length is not supported.
    InvalidHeaderLength(u16),
    /// The header checksum is invalid.
    HeaderIntegrity,
    /// Reserved bytes or flags are nonzero.
    NonzeroReserved,
    /// The codec tag is unknown.
    UnknownCodec(u8),
    /// The profile tag is unknown.
    UnknownProfile(u8),
    /// The executable format tag is unknown.
    UnknownFormat(u8),
    /// The executable class tag is unknown.
    UnknownClass(u8),
    /// The byte-order tag is unknown.
    UnknownEndianness(u8),
    /// Embedded executable metadata is outside the stable tier.
    UnsupportedEmbeddedMetadata(&'static str),
    /// Header and actual file lengths disagree.
    InvalidContainerLength {
        /// Length declared by the header.
        declared: u64,
        /// Actual file length.
        actual: u64,
    },
    /// The compressed payload checksum is invalid.
    PayloadIntegrity,
    /// Compression failed.
    Compression(String),
    /// Decompression failed.
    Decompression(String),
    /// Decompressed length does not match the manifest.
    OriginalLength {
        /// Length declared by the header.
        declared: u64,
        /// Actual decompressed length.
        actual: u64,
    },
    /// The original executable checksum is invalid.
    OriginalIntegrity,
    /// Decompressed executable metadata differs from the authenticated header.
    MetadataMismatch(&'static str),
    /// Compression would not reduce the total artifact size.
    NotBeneficial {
        /// Original executable length.
        original: u64,
        /// Proposed container length.
        container: u64,
    },
    /// A benchmark iteration count is outside the bounded range.
    InvalidIterations {
        /// Requested iteration count.
        actual: u32,
        /// Maximum permitted count.
        maximum: u32,
    },
    /// Repeated packing produced different bytes for the same inputs.
    NonDeterministic(Profile),
}

impl fmt::Display for ContainerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => write!(formatter, "unsupported input: {error}"),
            Self::SizeLimit {
                field,
                actual,
                maximum,
            } => write!(
                formatter,
                "{field} is {actual} bytes; the M1 limit is {maximum} bytes"
            ),
            Self::TruncatedHeader => formatter.write_str("truncated Packforge container header"),
            Self::InvalidMagic => formatter.write_str("input is not a Packforge container"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported Packforge container version {version}"
                )
            }
            Self::InvalidHeaderLength(length) => {
                write!(formatter, "unsupported Packforge header length {length}")
            }
            Self::HeaderIntegrity => formatter.write_str("container header checksum mismatch"),
            Self::NonzeroReserved => {
                formatter.write_str("container uses unsupported flags or reserved bytes")
            }
            Self::UnknownCodec(tag) => write!(formatter, "unknown codec tag {tag}"),
            Self::UnknownProfile(tag) => write!(formatter, "unknown profile tag {tag}"),
            Self::UnknownFormat(tag) => write!(formatter, "unknown executable format tag {tag}"),
            Self::UnknownClass(tag) => write!(formatter, "unknown executable class tag {tag}"),
            Self::UnknownEndianness(tag) => {
                write!(formatter, "unknown executable byte-order tag {tag}")
            }
            Self::UnsupportedEmbeddedMetadata(field) => {
                write!(
                    formatter,
                    "unsupported embedded executable metadata: {field}"
                )
            }
            Self::InvalidContainerLength { declared, actual } => write!(
                formatter,
                "container length mismatch: header declares {declared} bytes, file has {actual}"
            ),
            Self::PayloadIntegrity => formatter.write_str("compressed payload checksum mismatch"),
            Self::Compression(message) => write!(formatter, "compression failed: {message}"),
            Self::Decompression(message) => write!(formatter, "decompression failed: {message}"),
            Self::OriginalLength { declared, actual } => write!(
                formatter,
                "decompressed length mismatch: header declares {declared} bytes, got {actual}"
            ),
            Self::OriginalIntegrity => {
                formatter.write_str("decompressed executable checksum mismatch")
            }
            Self::MetadataMismatch(field) => write!(
                formatter,
                "decompressed executable metadata mismatch in {field}"
            ),
            Self::NotBeneficial {
                original,
                container,
            } => write!(
                formatter,
                "packing would grow the file from {original} to {container} bytes; pass --allow-larger to keep it"
            ),
            Self::InvalidIterations { actual, maximum } => write!(
                formatter,
                "benchmark iterations must be between 1 and {maximum}; got {actual}"
            ),
            Self::NonDeterministic(profile) => write!(
                formatter,
                "{} profile produced non-deterministic container bytes",
                profile.as_str()
            ),
        }
    }
}

impl std::error::Error for ContainerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            _ => None,
        }
    }
}

impl From<FormatError> for ContainerError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}

#[derive(Debug, Clone)]
struct Header {
    codec: Codec,
    profile: Profile,
    codec_level: i32,
    original_size: u64,
    payload_size: u64,
    original_hash: [u8; 32],
    payload_hash: [u8; 32],
    original_mode: u32,
    binary: BinaryInfo,
}

struct ParsedContainer<'a> {
    header: Header,
    payload: &'a [u8],
}

/// Packs a supported executable into a deterministic, reversible container.
///
/// # Errors
///
/// Returns [`ContainerError`] when the input is too large, outside the current
/// executable tier, cannot be compressed, or would grow without explicit opt-in.
pub fn pack(
    input: &[u8],
    original_mode: u32,
    options: PackOptions,
) -> Result<PackedArtifact, ContainerError> {
    enforce_size("original image", input.len(), MAX_ORIGINAL_SIZE)?;
    let binary = classify(input)?;
    let (codec, codec_level, payload) = compress(input, options.profile)?;
    enforce_size("compressed payload", payload.len(), MAX_PAYLOAD_SIZE)?;

    let original_hash = *blake3::hash(input).as_bytes();
    let payload_hash = *blake3::hash(&payload).as_bytes();
    let header = Header {
        codec,
        profile: options.profile,
        codec_level,
        original_size: usize_to_u64(input.len()),
        payload_size: usize_to_u64(payload.len()),
        original_hash,
        payload_hash,
        original_mode,
        binary,
    };
    let encoded_header = encode_header(&header);
    let container_size =
        HEADER_LEN
            .checked_add(payload.len())
            .ok_or(ContainerError::SizeLimit {
                field: "container",
                actual: u64::MAX,
                maximum: MAX_CONTAINER_SIZE,
            })?;
    let container_size_u64 = usize_to_u64(container_size);
    if !options.allow_larger && container_size >= input.len() {
        return Err(ContainerError::NotBeneficial {
            original: usize_to_u64(input.len()),
            container: container_size_u64,
        });
    }

    let mut bytes = Vec::with_capacity(container_size);
    bytes.extend_from_slice(&encoded_header);
    bytes.extend_from_slice(&payload);
    let info = artifact_info(&header, Verification::Full);
    Ok(PackedArtifact { bytes, info })
}

/// Validates header and compressed-payload integrity without decompressing.
///
/// # Errors
///
/// Returns [`ContainerError`] when framing, metadata, limits, or payload integrity
/// validation fails.
pub fn inspect(container: &[u8]) -> Result<ArtifactInfo, ContainerError> {
    let parsed = parse(container)?;
    Ok(artifact_info(&parsed.header, Verification::Payload))
}

/// Fully validates a container and its reconstructed executable.
///
/// # Errors
///
/// Returns [`ContainerError`] when container validation, bounded decompression,
/// original-image integrity, or executable reclassification fails.
pub fn verify(container: &[u8]) -> Result<ArtifactInfo, ContainerError> {
    let parsed = parse(container)?;
    let original = decompress(&parsed.header, parsed.payload)?;
    validate_original(&parsed.header, &original)?;
    Ok(artifact_info(&parsed.header, Verification::Full))
}

/// Fully validates and reconstructs the original executable.
///
/// # Errors
///
/// Returns [`ContainerError`] for the same validation and reconstruction failures
/// as [`verify`].
pub fn unpack(container: &[u8]) -> Result<UnpackedArtifact, ContainerError> {
    let parsed = parse(container)?;
    let original = decompress(&parsed.header, parsed.payload)?;
    validate_original(&parsed.header, &original)?;
    let info = artifact_info(&parsed.header, Verification::Full);
    Ok(UnpackedArtifact {
        bytes: original,
        info,
    })
}

fn compress(input: &[u8], profile: Profile) -> Result<(Codec, i32, Vec<u8>), ContainerError> {
    match profile {
        Profile::Fast => Ok((Codec::Lz4, 0, lz4_flex::block::compress(input))),
        Profile::Balanced => compress_zstd(input, 3),
        Profile::Small => compress_zstd(input, 19),
        Profile::Auto => {
            let mut candidates = vec![
                (Codec::Lz4, 0, lz4_flex::block::compress(input)),
                compress_zstd(input, 3)?,
                compress_zstd(input, 19)?,
            ];
            candidates.sort_by_key(|(codec, level, payload)| (payload.len(), codec.tag(), *level));
            Ok(candidates.remove(0))
        }
    }
}

fn compress_zstd(input: &[u8], level: i32) -> Result<(Codec, i32, Vec<u8>), ContainerError> {
    zstd::stream::encode_all(Cursor::new(input), level)
        .map(|payload| (Codec::Zstd, level, payload))
        .map_err(|error| ContainerError::Compression(error.to_string()))
}

fn parse(container: &[u8]) -> Result<ParsedContainer<'_>, ContainerError> {
    enforce_size("container", container.len(), MAX_CONTAINER_SIZE)?;
    let header_bytes = container
        .get(..HEADER_LEN)
        .ok_or(ContainerError::TruncatedHeader)?;
    let header = decode_header(header_bytes)?;
    let expected_size = HEADER_LEN
        .checked_add(u64_to_usize(header.payload_size, "compressed payload")?)
        .ok_or(ContainerError::InvalidContainerLength {
            declared: u64::MAX,
            actual: usize_to_u64(container.len()),
        })?;
    if expected_size != container.len() {
        return Err(ContainerError::InvalidContainerLength {
            declared: usize_to_u64(expected_size),
            actual: usize_to_u64(container.len()),
        });
    }
    let payload = &container[HEADER_LEN..];
    if blake3::hash(payload).as_bytes() != &header.payload_hash {
        return Err(ContainerError::PayloadIntegrity);
    }
    Ok(ParsedContainer { header, payload })
}

fn decompress(header: &Header, payload: &[u8]) -> Result<Vec<u8>, ContainerError> {
    let original_len = u64_to_usize(header.original_size, "original image")?;
    let output = match header.codec {
        Codec::Lz4 => {
            let mut output = vec![0u8; original_len];
            let written = lz4_flex::block::decompress_into(payload, &mut output)
                .map_err(|error| ContainerError::Decompression(error.to_string()))?;
            if written != output.len() {
                return Err(ContainerError::OriginalLength {
                    declared: header.original_size,
                    actual: usize_to_u64(written),
                });
            }
            output
        }
        Codec::Zstd => {
            let decoder = zstd::stream::read::Decoder::new(Cursor::new(payload))
                .map_err(|error| ContainerError::Decompression(error.to_string()))?;
            let mut decoder = decoder;
            decoder
                .window_log_max(ZSTD_MAX_WINDOW_LOG)
                .map_err(|error| ContainerError::Decompression(error.to_string()))?;
            let limit = header
                .original_size
                .checked_add(1)
                .ok_or(ContainerError::SizeLimit {
                    field: "original image",
                    actual: u64::MAX,
                    maximum: MAX_ORIGINAL_SIZE,
                })?;
            let mut output = Vec::with_capacity(original_len.min(1 << 20));
            decoder
                .take(limit)
                .read_to_end(&mut output)
                .map_err(|error| ContainerError::Decompression(error.to_string()))?;
            output
        }
    };

    if output.len() != original_len {
        return Err(ContainerError::OriginalLength {
            declared: header.original_size,
            actual: usize_to_u64(output.len()),
        });
    }
    Ok(output)
}

fn validate_original(header: &Header, original: &[u8]) -> Result<(), ContainerError> {
    if blake3::hash(original).as_bytes() != &header.original_hash {
        return Err(ContainerError::OriginalIntegrity);
    }
    let actual = classify(original)?;
    compare_metadata(header.binary, actual)
}

fn compare_metadata(expected: BinaryInfo, actual: BinaryInfo) -> Result<(), ContainerError> {
    if expected.format != actual.format {
        return Err(ContainerError::MetadataMismatch("format"));
    }
    if expected.class != actual.class {
        return Err(ContainerError::MetadataMismatch("class"));
    }
    if expected.endianness != actual.endianness {
        return Err(ContainerError::MetadataMismatch("endianness"));
    }
    if expected.machine != actual.machine {
        return Err(ContainerError::MetadataMismatch("machine"));
    }
    if expected.file_type != actual.file_type {
        return Err(ContainerError::MetadataMismatch("file_type"));
    }
    if expected.entry_point != actual.entry_point {
        return Err(ContainerError::MetadataMismatch("entry_point"));
    }
    if expected.load_segments != actual.load_segments {
        return Err(ContainerError::MetadataMismatch("load_segments"));
    }
    Ok(())
}

fn artifact_info(header: &Header, verification: Verification) -> ArtifactInfo {
    let ratio = u128::from(header.payload_size)
        .saturating_mul(10_000)
        .checked_div(u128::from(header.original_size))
        .unwrap_or(0);
    ArtifactInfo {
        schema_version: 1,
        container_version: CONTAINER_VERSION,
        verification,
        profile: header.profile,
        codec: header.codec,
        codec_level: header.codec_level,
        original_size: header.original_size,
        payload_size: header.payload_size,
        container_size: header.payload_size + HEADER_LEN_U64,
        payload_ratio_basis_points: u32::try_from(ratio).unwrap_or(u32::MAX),
        original_digest: hex(&header.original_hash),
        payload_digest: hex(&header.payload_hash),
        original_mode: header.original_mode,
        binary: header.binary,
    }
}

fn encode_header(header: &Header) -> [u8; HEADER_LEN] {
    let mut bytes = [0u8; HEADER_LEN];
    bytes[..8].copy_from_slice(MAGIC);
    put_u16(&mut bytes, 8, CONTAINER_VERSION);
    put_u16(&mut bytes, 10, HEADER_LEN_U16);
    bytes[12] = header.codec.tag();
    bytes[13] = header.profile.tag();
    bytes[14] = FORMAT_ELF;
    bytes[15] = CLASS_ELF64;
    bytes[16] = ENDIAN_LITTLE;
    bytes[17] = 0;
    put_u16(&mut bytes, 18, header.binary.machine);
    put_u16(&mut bytes, 20, header.binary.file_type);
    put_u16(&mut bytes, 22, header.binary.load_segments);
    put_u32(&mut bytes, 24, header.original_mode);
    put_i32(&mut bytes, 28, header.codec_level);
    put_u64(&mut bytes, 32, header.original_size);
    put_u64(&mut bytes, 40, header.payload_size);
    put_u64(&mut bytes, 48, header.binary.entry_point);
    let config_hash = config_hash(header);
    bytes[56..88].copy_from_slice(&config_hash);
    bytes[88..120].copy_from_slice(&header.original_hash);
    bytes[120..152].copy_from_slice(&header.payload_hash);
    let header_hash = *blake3::hash(&bytes).as_bytes();
    bytes[HEADER_HASH_OFFSET..HEADER_HASH_END].copy_from_slice(&header_hash);
    bytes
}

fn decode_header(bytes: &[u8]) -> Result<Header, ContainerError> {
    if bytes.get(..8) != Some(MAGIC) {
        return Err(ContainerError::InvalidMagic);
    }
    let version = get_u16(bytes, 8)?;
    if version != CONTAINER_VERSION {
        return Err(ContainerError::UnsupportedVersion(version));
    }
    let header_len = get_u16(bytes, 10)?;
    if usize::from(header_len) != HEADER_LEN {
        return Err(ContainerError::InvalidHeaderLength(header_len));
    }
    let stored_header_hash = bytes
        .get(HEADER_HASH_OFFSET..HEADER_HASH_END)
        .ok_or(ContainerError::TruncatedHeader)?;
    let mut hash_input = [0u8; HEADER_LEN];
    hash_input.copy_from_slice(bytes);
    hash_input[HEADER_HASH_OFFSET..HEADER_HASH_END].fill(0);
    if blake3::hash(&hash_input).as_bytes().as_slice() != stored_header_hash {
        return Err(ContainerError::HeaderIntegrity);
    }
    if bytes[17] != 0 || bytes[RESERVED_OFFSET..].iter().any(|byte| *byte != 0) {
        return Err(ContainerError::NonzeroReserved);
    }

    let codec = Codec::from_tag(bytes[12])?;
    let profile = Profile::from_tag(bytes[13])?;
    if bytes[14] != FORMAT_ELF {
        return Err(ContainerError::UnknownFormat(bytes[14]));
    }
    if bytes[15] != CLASS_ELF64 {
        return Err(ContainerError::UnknownClass(bytes[15]));
    }
    if bytes[16] != ENDIAN_LITTLE {
        return Err(ContainerError::UnknownEndianness(bytes[16]));
    }
    let machine = get_u16(bytes, 18)?;
    if machine != MACHINE_X86_64 {
        return Err(ContainerError::UnsupportedEmbeddedMetadata("machine"));
    }
    let file_type = get_u16(bytes, 20)?;
    if file_type != FILE_TYPE_EXECUTABLE {
        return Err(ContainerError::UnsupportedEmbeddedMetadata("file_type"));
    }
    let load_segments = get_u16(bytes, 22)?;
    if load_segments == 0 {
        return Err(ContainerError::UnsupportedEmbeddedMetadata("load_segments"));
    }
    let original_mode = get_u32(bytes, 24)?;
    let codec_level = get_i32(bytes, 28)?;
    validate_codec_level(codec, codec_level)?;
    let original_size = get_u64(bytes, 32)?;
    enforce_u64_size("original image", original_size, MAX_ORIGINAL_SIZE)?;
    let payload_size = get_u64(bytes, 40)?;
    enforce_u64_size("compressed payload", payload_size, MAX_PAYLOAD_SIZE)?;
    let entry_point = get_u64(bytes, 48)?;
    let original_hash = get_array_32(bytes, 88)?;
    let payload_hash = get_array_32(bytes, 120)?;
    let header = Header {
        codec,
        profile,
        codec_level,
        original_size,
        payload_size,
        original_hash,
        payload_hash,
        original_mode,
        binary: BinaryInfo {
            format: BinaryFormat::Elf,
            class: BinaryClass::Elf64,
            endianness: Endianness::Little,
            architecture: Architecture::X86_64,
            binary_type: BinaryType::StaticExecutable,
            machine,
            file_type,
            entry_point,
            load_segments,
        },
    };
    if bytes[56..88] != config_hash(&header) {
        return Err(ContainerError::HeaderIntegrity);
    }
    Ok(header)
}

fn validate_codec_level(codec: Codec, level: i32) -> Result<(), ContainerError> {
    match codec {
        Codec::Lz4 if level == 0 => Ok(()),
        Codec::Zstd if (-7..=22).contains(&level) => Ok(()),
        Codec::Lz4 => Err(ContainerError::UnsupportedEmbeddedMetadata(
            "LZ4 codec level",
        )),
        Codec::Zstd => Err(ContainerError::UnsupportedEmbeddedMetadata(
            "Zstandard codec level",
        )),
    }
}

fn config_hash(header: &Header) -> [u8; 32] {
    let mut config = [0u8; 32];
    put_u16(&mut config, 0, CONTAINER_VERSION);
    config[2] = header.codec.tag();
    config[3] = header.profile.tag();
    config[4] = FORMAT_ELF;
    config[5] = CLASS_ELF64;
    config[6] = ENDIAN_LITTLE;
    put_u16(&mut config, 8, header.binary.machine);
    put_u16(&mut config, 10, header.binary.file_type);
    put_u16(&mut config, 12, header.binary.load_segments);
    put_i32(&mut config, 16, header.codec_level);
    put_u64(&mut config, 20, header.binary.entry_point);
    *blake3::hash(&config).as_bytes()
}

fn enforce_size(field: &'static str, size: usize, maximum: u64) -> Result<(), ContainerError> {
    enforce_u64_size(field, usize_to_u64(size), maximum)
}

fn enforce_u64_size(field: &'static str, size: u64, maximum: u64) -> Result<(), ContainerError> {
    if size == 0 || size > maximum {
        return Err(ContainerError::SizeLimit {
            field,
            actual: size,
            maximum,
        });
    }
    Ok(())
}

fn u64_to_usize(value: u64, field: &'static str) -> Result<usize, ContainerError> {
    usize::try_from(value).map_err(|_| ContainerError::SizeLimit {
        field,
        actual: value,
        maximum: usize_to_u64(usize::MAX),
    })
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i32(output: &mut [u8], offset: usize, value: i32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(input: &[u8], offset: usize) -> Result<u16, ContainerError> {
    let bytes = input
        .get(offset..offset + 2)
        .ok_or(ContainerError::TruncatedHeader)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn get_u32(input: &[u8], offset: usize) -> Result<u32, ContainerError> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or(ContainerError::TruncatedHeader)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn get_i32(input: &[u8], offset: usize) -> Result<i32, ContainerError> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or(ContainerError::TruncatedHeader)?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn get_u64(input: &[u8], offset: usize) -> Result<u64, ContainerError> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or(ContainerError::TruncatedHeader)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn get_array_32(input: &[u8], offset: usize) -> Result<[u8; 32], ContainerError> {
    input
        .get(offset..offset + 32)
        .ok_or(ContainerError::TruncatedHeader)?
        .try_into()
        .map_err(|_| ContainerError::TruncatedHeader)
}

#[cfg(test)]
mod tests {
    use super::{
        ContainerError, HEADER_HASH_END, HEADER_HASH_OFFSET, HEADER_LEN, PackOptions, Profile,
        Verification, inspect, pack, unpack, verify,
    };

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 16_384];
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
        bytes[72..80].copy_from_slice(&0u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&16_384u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&16_384u64.to_le_bytes());
        bytes[256..].fill(0x41);
        bytes
    }

    #[test]
    fn every_profile_round_trips() {
        let original = fixture();
        for profile in [
            Profile::Fast,
            Profile::Balanced,
            Profile::Small,
            Profile::Auto,
        ] {
            let packed = pack(
                &original,
                0o755,
                PackOptions {
                    profile,
                    allow_larger: false,
                },
            )
            .expect("fixture should pack");
            assert_eq!(
                inspect(&packed.bytes).unwrap().verification,
                Verification::Payload
            );
            assert_eq!(
                verify(&packed.bytes).unwrap().verification,
                Verification::Full
            );
            let unpacked = unpack(&packed.bytes).expect("fixture should unpack");
            assert_eq!(unpacked.bytes, original);
            assert_eq!(unpacked.info.original_mode, 0o755);
        }
    }

    #[test]
    fn output_is_deterministic() {
        let original = fixture();
        let options = PackOptions {
            profile: Profile::Auto,
            allow_larger: false,
        };
        let first = pack(&original, 0o755, options).unwrap();
        let second = pack(&original, 0o755, options).unwrap();
        assert_eq!(first.bytes, second.bytes);
    }

    #[test]
    fn payload_corruption_is_detected_before_decompression() {
        let original = fixture();
        let mut packed = pack(&original, 0o755, PackOptions::default())
            .unwrap()
            .bytes;
        packed[HEADER_LEN] ^= 0x80;
        assert!(matches!(
            inspect(&packed),
            Err(ContainerError::PayloadIntegrity)
        ));
    }

    #[test]
    fn header_corruption_is_detected() {
        let original = fixture();
        let mut packed = pack(&original, 0o755, PackOptions::default())
            .unwrap()
            .bytes;
        packed[32] ^= 0x01;
        assert!(matches!(
            inspect(&packed),
            Err(ContainerError::HeaderIntegrity)
        ));
    }

    #[test]
    fn truncated_container_is_rejected() {
        assert!(matches!(
            inspect(&[0u8; HEADER_LEN - 1]),
            Err(ContainerError::TruncatedHeader)
        ));
    }

    #[test]
    fn non_beneficial_output_requires_opt_in() {
        let mut original = fixture();
        let mut state = 0x8f3d_2a19u32;
        for byte in &mut original[256..] {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state.to_le_bytes()[0];
        }
        let result = pack(
            &original,
            0o755,
            PackOptions {
                profile: Profile::Fast,
                allow_larger: false,
            },
        );
        assert!(matches!(result, Err(ContainerError::NotBeneficial { .. })));
    }

    #[test]
    fn header_hash_field_is_nonzero() {
        let packed = pack(&fixture(), 0o755, PackOptions::default())
            .unwrap()
            .bytes;
        assert!(
            packed[HEADER_HASH_OFFSET..HEADER_HASH_END]
                .iter()
                .any(|byte| *byte != 0)
        );
    }
}
