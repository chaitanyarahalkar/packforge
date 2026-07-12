//! Host-side encoding for the M2 direct-load executable format.

use std::fmt;

use packforge_lzma_decoder as lzma;
use serde::Serialize;

use crate::MAX_ORIGINAL_SIZE;
use crate::container::{HEADER_LEN, MAX_CONTAINER_SIZE, PackOptions, Profile, Verification};
use crate::executable::EXECUTABLE_TRAILER_LEN;
use crate::executable_v2_codec4::{self as codec4, CODEC_LZMA1_BCJ4};
use crate::manifest::{
    MANIFEST_HEADER_LEN, MANIFEST_SEGMENT_LEN, MAX_MANIFEST_SEGMENTS, ManifestElfError,
    ManifestError, ManifestV0, decode_manifest_v0, manifest_from_elf,
};

/// Direct-load executable format version.
pub const EXECUTABLE_V2_VERSION: u16 = 2;
/// Direct-load runtime ABI version.
pub const RUNTIME_V2_ABI_VERSION: u16 = 2;
/// Fixed direct-load image header length.
pub const EXECUTABLE_V2_HEADER_LEN: usize = 192;
const EXECUTABLE_V2_HEADER_LEN_U16: u16 = 192;
const EXECUTABLE_TRAILER_LEN_U16: u16 = 128;
/// M2 complete-loader size gate.
pub const MAX_RUNTIME_V2_SIZE: u64 = 23_500;
/// Maximum accepted direct-load executable length.
pub const MAX_EXECUTABLE_V2_SIZE: u64 = MAX_RUNTIME_V2_SIZE
    + EXECUTABLE_V2_HEADER_LEN as u64
    + (MANIFEST_HEADER_LEN + MAX_MANIFEST_SEGMENTS * MANIFEST_SEGMENT_LEN) as u64
    + (MAX_CONTAINER_SIZE - HEADER_LEN as u64)
    + EXECUTABLE_TRAILER_LEN as u64;

const HEADER_MAGIC: &[u8; 8] = b"PFGIMG02";
const TRAILER_MAGIC: &[u8; 8] = b"PFGEXE02";
const CODEC_LZMA1: u16 = 3;
const HEADER_HASH_OFFSET: usize = 160;
const TRAILER_HASH_OFFSET: usize = 96;
const TARGET_LINUX: u16 = 1;
const TARGET_X86_64: u16 = 62;
const MIN_DICTIONARY_SIZE: u32 = 1 << 12;
const MAX_DICTIONARY_SIZE: u32 = 1 << 26;
const FIXED_LZMA_PROPERTIES: u8 = 0x5d;
const PAGE_SIZE: u64 = 4096;

/// Reproducible fail-closed `ET_DYN` artifact used while direct mapping is built.
pub const LINUX_X86_64_RUNTIME_V2: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../runtime/artifacts/linux-x86_64/loader-v2"
));

/// Stable metadata returned for an experimental executable v2 artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutableV2Info {
    /// JSON/report schema version.
    pub schema_version: u16,
    /// Executable wrapper version.
    pub executable_version: u16,
    /// Native loader ABI version.
    pub runtime_abi_version: u16,
    /// Validation depth reached.
    pub verification: Verification,
    /// Native loader length.
    pub loader_size: u64,
    /// Canonical manifest length.
    pub manifest_size: u64,
    /// Raw LZMA1 payload length.
    pub payload_size: u64,
    /// Original executable length.
    pub original_size: u64,
    /// Complete packed executable length.
    pub executable_size: u64,
    /// Original Unix mode.
    pub original_mode: u32,
    /// Expected range-coder flush bytes.
    pub trailing_bytes: u8,
    /// BLAKE3 digest of the native loader.
    pub loader_digest: String,
    /// BLAKE3 digest of the canonical manifest.
    pub manifest_digest: String,
    /// BLAKE3 digest of the compressed payload.
    pub payload_digest: String,
    /// BLAKE3 digest of the complete original executable.
    pub original_digest: String,
    /// Canonical direct-mapping description.
    pub manifest: ManifestV0,
}

/// Bytes and metadata produced by experimental v2 packing.
#[derive(Debug)]
pub struct PackedExecutableV2 {
    /// Complete self-contained executable bytes.
    pub bytes: Vec<u8>,
    /// Fully verified wrapper metadata.
    pub info: ExecutableV2Info,
}

/// Bytes and metadata produced by experimental v2 unpacking.
#[derive(Debug)]
pub struct UnpackedExecutableV2 {
    /// Byte-identical original executable.
    pub bytes: Vec<u8>,
    /// Fully verified wrapper metadata.
    pub info: ExecutableV2Info,
}

/// Errors produced by bounded executable v2 host processing.
#[derive(Debug)]
pub enum ExecutableV2Error {
    /// Source ELF parsing or manifest derivation failed.
    ManifestElf(ManifestElfError),
    /// Embedded manifest validation failed.
    Manifest(ManifestError),
    /// The selected profile is not the M2 released profile.
    UnsupportedProfile(Profile),
    /// Loader framing or size is invalid.
    InvalidLoader,
    /// The final trailer is missing.
    TruncatedTrailer,
    /// Header or trailer magic is invalid.
    InvalidMagic,
    /// A framing version or ABI is unsupported.
    UnsupportedVersion(u16),
    /// A fixed structure length is invalid.
    InvalidStructureLength(u16),
    /// Flags, target, codec properties, or reserved fields are unsupported.
    UnsupportedMetadata,
    /// A checked length or range is inconsistent.
    InvalidRange,
    /// A digest does not match its exact range.
    Integrity(&'static str),
    /// Raw LZMA1 decoding failed.
    Decompression(lzma::DecodeError),
    /// The decoder consumed a noncanonical trailing-byte count.
    TrailingBytes { expected: u8, actual: u8 },
    /// Recovered ELF metadata differs from the authenticated manifest.
    ManifestMismatch,
    /// Load segments cannot share one bounded direct-output address span.
    UnsupportedDirectOutputLayout,
    /// The complete executable would not reduce artifact size.
    NotBeneficial { original: u64, executable: u64 },
    /// A host length cannot be represented safely.
    SizeOverflow(&'static str),
}

impl fmt::Display for ExecutableV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestElf(error) => error.fmt(formatter),
            Self::Manifest(error) => error.fmt(formatter),
            Self::UnsupportedProfile(profile) => write!(
                formatter,
                "profile {} is not available for executable v2; use balanced",
                profile.as_str()
            ),
            Self::InvalidLoader => formatter.write_str("invalid executable v2 loader"),
            Self::TruncatedTrailer => formatter.write_str("truncated executable v2 trailer"),
            Self::InvalidMagic => formatter.write_str("input is not a Packforge executable v2"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported executable v2 version {version}")
            }
            Self::InvalidStructureLength(length) => {
                write!(
                    formatter,
                    "unsupported executable v2 structure length {length}"
                )
            }
            Self::UnsupportedMetadata => {
                formatter.write_str("executable v2 uses unsupported metadata")
            }
            Self::InvalidRange => formatter.write_str("executable v2 range is invalid"),
            Self::Integrity(range) => write!(formatter, "executable v2 {range} digest mismatch"),
            Self::Decompression(error) => write!(formatter, "LZMA1 decoding failed: {error:?}"),
            Self::TrailingBytes { expected, actual } => write!(
                formatter,
                "LZMA1 trailing-byte mismatch: expected {expected}, decoded {actual}"
            ),
            Self::ManifestMismatch => {
                formatter.write_str("recovered ELF does not match its authenticated manifest")
            }
            Self::UnsupportedDirectOutputLayout => formatter
                .write_str("executable v2 load segments do not share a bounded output layout"),
            Self::NotBeneficial {
                original,
                executable,
            } => write!(
                formatter,
                "self-contained executable would grow from {original} to {executable} bytes; pass --allow-larger to keep it"
            ),
            Self::SizeOverflow(field) => write!(formatter, "{field} length cannot be represented"),
        }
    }
}

impl std::error::Error for ExecutableV2Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ManifestElf(error) => Some(error),
            Self::Manifest(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ManifestElfError> for ExecutableV2Error {
    fn from(error: ManifestElfError) -> Self {
        Self::ManifestElf(error)
    }
}

impl From<ManifestError> for ExecutableV2Error {
    fn from(error: ManifestError) -> Self {
        Self::Manifest(error)
    }
}

#[derive(Debug, Clone)]
struct ImageHeader {
    codec: u16,
    original_mode: u32,
    properties: [u8; 5],
    trailing_bytes: u8,
    manifest_length: u64,
    payload_length: u64,
    original_length: u64,
    original_digest: [u8; 32],
    manifest_digest: [u8; 32],
    payload_digest: [u8; 32],
}

#[derive(Debug)]
struct Trailer {
    image_offset: u64,
    image_length: u64,
    executable_length: u64,
    loader_length: u64,
    loader_digest: [u8; 32],
}

struct ParsedExecutable<'a> {
    payload: &'a [u8],
    header: ImageHeader,
    trailer: Trailer,
    manifest: ManifestV0,
}

/// Encodes an experimental direct-load executable v2 artifact.
///
/// # Errors
///
/// Returns [`ExecutableV2Error`] for unsupported input, profile, loader,
/// compression metadata, bounds, or a non-beneficial result without opt-in.
pub fn pack_executable_v2(
    original: &[u8],
    original_mode: u32,
    options: PackOptions,
    loader: &[u8],
) -> Result<PackedExecutableV2, ExecutableV2Error> {
    pack_executable_v2_with_codec(original, original_mode, options, loader, CODEC_LZMA1)
}

/// Packs codec 4 with an explicitly supplied candidate loader for M2 validation.
///
/// This entry point is intentionally excluded from the released CLI selection
/// until the assembly runtime passes every M2 gate.
#[doc(hidden)]
pub fn pack_executable_v2_codec4(
    original: &[u8],
    original_mode: u32,
    options: PackOptions,
    loader: &[u8],
) -> Result<PackedExecutableV2, ExecutableV2Error> {
    pack_executable_v2_with_codec(original, original_mode, options, loader, CODEC_LZMA1_BCJ4)
}

fn pack_executable_v2_with_codec(
    original: &[u8],
    original_mode: u32,
    options: PackOptions,
    loader: &[u8],
    codec: u16,
) -> Result<PackedExecutableV2, ExecutableV2Error> {
    if options.profile != Profile::Balanced {
        return Err(ExecutableV2Error::UnsupportedProfile(options.profile));
    }
    validate_loader(loader)?;
    let manifest = manifest_from_elf(original)?;
    validate_direct_output_layout(&manifest)?;
    let manifest_bytes = manifest.encode()?;
    let original_length = usize_to_u64(original.len(), "original executable")?;
    let reduce_size = u32::try_from(original.len())
        .map_err(|_| ExecutableV2Error::SizeOverflow("original executable"))?;
    let properties = lzma_sdk_rs::LzmaProps::for_level(9, reduce_size);
    let decoder_properties = lzma_sdk_rs::decoder_props(&properties);
    let (payload, trailing_bytes, decoded) = match codec {
        CODEC_LZMA1 => {
            let payload = lzma_sdk_rs::encode(original, &properties);
            let mut decoded = vec![0u8; original.len()];
            let report = lzma::decompress(&payload, &decoder_properties, &mut decoded)
                .map_err(ExecutableV2Error::Decompression)?;
            (payload, report.trailing_bytes, decoded)
        }
        CODEC_LZMA1_BCJ4 => {
            let payload = codec4::encode(original, &properties)?;
            let decoded = codec4::decode(&payload, decoder_properties, original.len())?;
            (payload, 0, decoded)
        }
        _ => return Err(ExecutableV2Error::UnsupportedMetadata),
    };
    validate_properties(codec, decoder_properties, trailing_bytes)?;
    if decoded != original {
        return Err(ExecutableV2Error::Integrity("original"));
    }
    let header = ImageHeader {
        codec,
        original_mode,
        properties: decoder_properties,
        trailing_bytes,
        manifest_length: usize_to_u64(manifest_bytes.len(), "manifest")?,
        payload_length: usize_to_u64(payload.len(), "payload")?,
        original_length,
        original_digest: digest(original),
        manifest_digest: digest(&manifest_bytes),
        payload_digest: digest(&payload),
    };
    let header_bytes = encode_header(&header);
    let loader_length = usize_to_u64(loader.len(), "loader")?;
    let image_length = (EXECUTABLE_V2_HEADER_LEN as u64)
        .checked_add(header.manifest_length)
        .and_then(|length| length.checked_add(header.payload_length))
        .ok_or(ExecutableV2Error::InvalidRange)?;
    let executable_length = loader_length
        .checked_add(image_length)
        .and_then(|length| length.checked_add(EXECUTABLE_TRAILER_LEN as u64))
        .ok_or(ExecutableV2Error::InvalidRange)?;
    if executable_length > MAX_EXECUTABLE_V2_SIZE {
        return Err(ExecutableV2Error::InvalidRange);
    }
    if !options.allow_larger && executable_length >= original_length {
        return Err(ExecutableV2Error::NotBeneficial {
            original: original_length,
            executable: executable_length,
        });
    }
    let trailer = Trailer {
        image_offset: loader_length,
        image_length,
        executable_length,
        loader_length,
        loader_digest: digest(loader),
    };
    let mut bytes = Vec::with_capacity(
        usize::try_from(executable_length)
            .map_err(|_| ExecutableV2Error::SizeOverflow("executable"))?,
    );
    bytes.extend_from_slice(loader);
    bytes.extend_from_slice(&header_bytes);
    bytes.extend_from_slice(&manifest_bytes);
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&encode_trailer(&trailer));
    Ok(PackedExecutableV2 {
        info: build_info(&header, &trailer, manifest, Verification::Full),
        bytes,
    })
}

/// Inspects all authenticated v2 ranges without decompressing the original.
///
/// # Errors
///
/// Returns [`ExecutableV2Error`] for malformed or inconsistent framing,
/// metadata, manifests, bounds, or digests.
pub fn inspect_executable_v2(executable: &[u8]) -> Result<ExecutableV2Info, ExecutableV2Error> {
    let parsed = parse(executable)?;
    Ok(build_info(
        &parsed.header,
        &parsed.trailer,
        parsed.manifest,
        Verification::Payload,
    ))
}

/// Fully verifies a v2 artifact and its recovered ELF manifest.
///
/// # Errors
///
/// Returns [`ExecutableV2Error`] for any framing, decompression, digest, or
/// recovered-manifest mismatch.
pub fn verify_executable_v2(executable: &[u8]) -> Result<ExecutableV2Info, ExecutableV2Error> {
    let parsed = parse(executable)?;
    let original = decompress_original(&parsed)?;
    validate_recovered_manifest(&original, &parsed.manifest)?;
    Ok(build_info(
        &parsed.header,
        &parsed.trailer,
        parsed.manifest,
        Verification::Full,
    ))
}

/// Recovers the byte-identical original from a fully verified v2 artifact.
///
/// # Errors
///
/// Returns [`ExecutableV2Error`] for any verification failure.
pub fn unpack_executable_v2(executable: &[u8]) -> Result<UnpackedExecutableV2, ExecutableV2Error> {
    let parsed = parse(executable)?;
    let original = decompress_original(&parsed)?;
    validate_recovered_manifest(&original, &parsed.manifest)?;
    Ok(UnpackedExecutableV2 {
        bytes: original,
        info: build_info(
            &parsed.header,
            &parsed.trailer,
            parsed.manifest,
            Verification::Full,
        ),
    })
}

fn parse(executable: &[u8]) -> Result<ParsedExecutable<'_>, ExecutableV2Error> {
    let executable_length = usize_to_u64(executable.len(), "executable")?;
    if executable_length > MAX_EXECUTABLE_V2_SIZE {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let trailer_offset = executable
        .len()
        .checked_sub(EXECUTABLE_TRAILER_LEN)
        .ok_or(ExecutableV2Error::TruncatedTrailer)?;
    let trailer = decode_trailer(&executable[trailer_offset..])?;
    if trailer.executable_length != executable_length
        || trailer.image_offset != trailer.loader_length
    {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let image_end = trailer
        .image_offset
        .checked_add(trailer.image_length)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    if image_end != usize_to_u64(trailer_offset, "trailer offset")? {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let loader_end = to_usize(trailer.loader_length, "loader")?;
    let loader = executable
        .get(..loader_end)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    validate_loader(loader)?;
    if digest(loader) != trailer.loader_digest {
        return Err(ExecutableV2Error::Integrity("loader"));
    }
    let header_end = loader_end
        .checked_add(EXECUTABLE_V2_HEADER_LEN)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    let header = decode_header(
        executable
            .get(loader_end..header_end)
            .ok_or(ExecutableV2Error::InvalidRange)?,
    )?;
    validate_properties(header.codec, header.properties, header.trailing_bytes)?;
    let manifest_end = header_end
        .checked_add(to_usize(header.manifest_length, "manifest")?)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    let payload_end = manifest_end
        .checked_add(to_usize(header.payload_length, "payload")?)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    if payload_end != trailer_offset {
        return Err(ExecutableV2Error::InvalidRange);
    }
    let manifest_bytes = executable
        .get(header_end..manifest_end)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    let payload = executable
        .get(manifest_end..payload_end)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    if digest(manifest_bytes) != header.manifest_digest {
        return Err(ExecutableV2Error::Integrity("manifest"));
    }
    if digest(payload) != header.payload_digest {
        return Err(ExecutableV2Error::Integrity("payload"));
    }
    let manifest = decode_manifest_v0(manifest_bytes)?;
    if manifest.original_size != header.original_length {
        return Err(ExecutableV2Error::ManifestMismatch);
    }
    validate_direct_output_layout(&manifest)?;
    Ok(ParsedExecutable {
        payload,
        header,
        trailer,
        manifest,
    })
}

fn decompress_original(parsed: &ParsedExecutable<'_>) -> Result<Vec<u8>, ExecutableV2Error> {
    let original_length = to_usize(parsed.header.original_length, "original")?;
    let original = match parsed.header.codec {
        CODEC_LZMA1 => {
            let mut original = vec![0u8; original_length];
            let report = lzma::decompress(parsed.payload, &parsed.header.properties, &mut original)
                .map_err(ExecutableV2Error::Decompression)?;
            if report.trailing_bytes != parsed.header.trailing_bytes {
                return Err(ExecutableV2Error::TrailingBytes {
                    expected: parsed.header.trailing_bytes,
                    actual: report.trailing_bytes,
                });
            }
            original
        }
        CODEC_LZMA1_BCJ4 => {
            codec4::decode(parsed.payload, parsed.header.properties, original_length)?
        }
        _ => return Err(ExecutableV2Error::UnsupportedMetadata),
    };
    if digest(&original) != parsed.header.original_digest {
        return Err(ExecutableV2Error::Integrity("original"));
    }
    Ok(original)
}

fn validate_recovered_manifest(
    original: &[u8],
    manifest: &ManifestV0,
) -> Result<(), ExecutableV2Error> {
    if manifest_from_elf(original)? != *manifest {
        return Err(ExecutableV2Error::ManifestMismatch);
    }
    Ok(())
}

fn validate_loader(loader: &[u8]) -> Result<(), ExecutableV2Error> {
    let length = usize_to_u64(loader.len(), "loader")?;
    if length == 0
        || length > MAX_RUNTIME_V2_SIZE
        || loader.get(..4) != Some(b"\x7fELF")
        || loader.get(4) != Some(&2)
        || loader.get(5) != Some(&1)
        || loader.get(18..20) != Some(&TARGET_X86_64.to_le_bytes())
    {
        return Err(ExecutableV2Error::InvalidLoader);
    }
    Ok(())
}

fn validate_direct_output_layout(manifest: &ManifestV0) -> Result<(), ExecutableV2Error> {
    let first = manifest
        .segments
        .first()
        .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
    let file_start = first
        .virtual_address
        .checked_sub(first.file_offset)
        .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
    if file_start < PAGE_SIZE || file_start & (PAGE_SIZE - 1) != 0 {
        return Err(ExecutableV2Error::UnsupportedDirectOutputLayout);
    }
    let mut start = file_start;
    let mut end = file_start
        .checked_add(manifest.original_size)
        .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
    let mut previous_map_end = 0u64;
    let mut previous_source_end = 0u64;
    let mut forward_destination_end = 0u64;
    for segment in &manifest.segments {
        let map_start = segment.virtual_address & !(PAGE_SIZE - 1);
        let map_end = segment
            .virtual_address
            .checked_add(segment.memory_size)
            .and_then(|value| value.checked_add(PAGE_SIZE - 1))
            .map(|value| value & !(PAGE_SIZE - 1))
            .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
        let source_start = file_start
            .checked_add(segment.file_offset)
            .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
        let source_end = source_start
            .checked_add(segment.file_size)
            .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
        let destination_end = segment
            .virtual_address
            .checked_add(segment.file_size)
            .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
        if map_start < previous_map_end
            || source_start < previous_source_end
            || forward_destination_end > source_start
            || (segment.virtual_address < source_start
                && segment.virtual_address < previous_source_end)
        {
            return Err(ExecutableV2Error::UnsupportedDirectOutputLayout);
        }
        forward_destination_end = if segment.virtual_address > source_start {
            destination_end
        } else {
            0
        };
        previous_map_end = map_end;
        previous_source_end = source_end;
        start = start.min(map_start);
        end = end.max(
            segment
                .virtual_address
                .checked_add(segment.memory_size)
                .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?,
        );
    }
    let end = end
        .checked_add(PAGE_SIZE - 1)
        .map(|value| value & !(PAGE_SIZE - 1))
        .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
    let length = end
        .checked_sub(start)
        .ok_or(ExecutableV2Error::UnsupportedDirectOutputLayout)?;
    if length == 0 || length > MAX_ORIGINAL_SIZE {
        return Err(ExecutableV2Error::UnsupportedDirectOutputLayout);
    }
    Ok(())
}

fn validate_properties(
    codec: u16,
    properties: [u8; 5],
    trailing: u8,
) -> Result<(), ExecutableV2Error> {
    let dictionary =
        u32::from_le_bytes([properties[1], properties[2], properties[3], properties[4]]);
    if properties[0] != FIXED_LZMA_PROPERTIES
        || !(MIN_DICTIONARY_SIZE..=MAX_DICTIONARY_SIZE).contains(&dictionary)
        || !matches!(codec, CODEC_LZMA1 | CODEC_LZMA1_BCJ4)
        || (codec == CODEC_LZMA1 && trailing > 5)
        || (codec == CODEC_LZMA1_BCJ4 && trailing != 0)
    {
        return Err(ExecutableV2Error::UnsupportedMetadata);
    }
    Ok(())
}

fn encode_header(header: &ImageHeader) -> [u8; EXECUTABLE_V2_HEADER_LEN] {
    let mut bytes = [0u8; EXECUTABLE_V2_HEADER_LEN];
    bytes[..8].copy_from_slice(HEADER_MAGIC);
    put_u16(&mut bytes, 8, EXECUTABLE_V2_VERSION);
    put_u16(&mut bytes, 10, EXECUTABLE_V2_HEADER_LEN_U16);
    put_u16(&mut bytes, 12, header.codec);
    put_u32(&mut bytes, 16, header.original_mode);
    bytes[20..25].copy_from_slice(&header.properties);
    bytes[25] = header.trailing_bytes;
    put_u64(&mut bytes, 32, header.manifest_length);
    put_u64(&mut bytes, 40, header.payload_length);
    put_u64(&mut bytes, 48, header.original_length);
    bytes[64..96].copy_from_slice(&header.original_digest);
    bytes[96..128].copy_from_slice(&header.manifest_digest);
    bytes[128..160].copy_from_slice(&header.payload_digest);
    let header_digest = digest(&bytes);
    bytes[HEADER_HASH_OFFSET..].copy_from_slice(&header_digest);
    bytes
}

fn decode_header(bytes: &[u8]) -> Result<ImageHeader, ExecutableV2Error> {
    if bytes.len() != EXECUTABLE_V2_HEADER_LEN {
        return Err(ExecutableV2Error::InvalidRange);
    }
    if bytes.get(..8) != Some(HEADER_MAGIC) {
        return Err(ExecutableV2Error::InvalidMagic);
    }
    require_version_and_length(bytes, EXECUTABLE_V2_HEADER_LEN)?;
    let stored_digest = array_32(bytes, HEADER_HASH_OFFSET)?;
    let mut hash_input = [0u8; EXECUTABLE_V2_HEADER_LEN];
    hash_input.copy_from_slice(bytes);
    hash_input[HEADER_HASH_OFFSET..].fill(0);
    if digest(&hash_input) != stored_digest {
        return Err(ExecutableV2Error::Integrity("header"));
    }
    let codec = get_u16(bytes, 12)?;
    if !matches!(codec, CODEC_LZMA1 | CODEC_LZMA1_BCJ4)
        || get_u16(bytes, 14)? != 0
        || bytes[26..32].iter().any(|byte| *byte != 0)
        || bytes[56..64].iter().any(|byte| *byte != 0)
    {
        return Err(ExecutableV2Error::UnsupportedMetadata);
    }
    Ok(ImageHeader {
        codec,
        original_mode: get_u32(bytes, 16)?,
        properties: bytes[20..25]
            .try_into()
            .map_err(|_| ExecutableV2Error::InvalidRange)?,
        trailing_bytes: bytes[25],
        manifest_length: get_u64(bytes, 32)?,
        payload_length: get_u64(bytes, 40)?,
        original_length: get_u64(bytes, 48)?,
        original_digest: array_32(bytes, 64)?,
        manifest_digest: array_32(bytes, 96)?,
        payload_digest: array_32(bytes, 128)?,
    })
}

fn encode_trailer(trailer: &Trailer) -> [u8; EXECUTABLE_TRAILER_LEN] {
    let mut bytes = [0u8; EXECUTABLE_TRAILER_LEN];
    bytes[..8].copy_from_slice(TRAILER_MAGIC);
    put_u16(&mut bytes, 8, EXECUTABLE_V2_VERSION);
    put_u16(&mut bytes, 10, EXECUTABLE_TRAILER_LEN_U16);
    put_u16(&mut bytes, 12, RUNTIME_V2_ABI_VERSION);
    put_u64(&mut bytes, 16, trailer.image_offset);
    put_u64(&mut bytes, 24, trailer.image_length);
    put_u64(&mut bytes, 32, trailer.executable_length);
    put_u64(&mut bytes, 40, trailer.loader_length);
    bytes[48..80].copy_from_slice(&trailer.loader_digest);
    put_u16(&mut bytes, 80, EXECUTABLE_V2_VERSION);
    put_u16(&mut bytes, 82, TARGET_LINUX);
    put_u16(&mut bytes, 84, TARGET_X86_64);
    let trailer_digest = digest(&bytes);
    bytes[TRAILER_HASH_OFFSET..].copy_from_slice(&trailer_digest);
    bytes
}

fn decode_trailer(bytes: &[u8]) -> Result<Trailer, ExecutableV2Error> {
    if bytes.len() != EXECUTABLE_TRAILER_LEN {
        return Err(ExecutableV2Error::TruncatedTrailer);
    }
    if bytes.get(..8) != Some(TRAILER_MAGIC) {
        return Err(ExecutableV2Error::InvalidMagic);
    }
    require_version_and_length(bytes, EXECUTABLE_TRAILER_LEN)?;
    let stored_digest = array_32(bytes, TRAILER_HASH_OFFSET)?;
    let mut hash_input = [0u8; EXECUTABLE_TRAILER_LEN];
    hash_input.copy_from_slice(bytes);
    hash_input[TRAILER_HASH_OFFSET..].fill(0);
    if digest(&hash_input) != stored_digest {
        return Err(ExecutableV2Error::Integrity("trailer"));
    }
    if get_u16(bytes, 12)? != RUNTIME_V2_ABI_VERSION
        || get_u16(bytes, 14)? != 0
        || get_u16(bytes, 80)? != EXECUTABLE_V2_VERSION
        || get_u16(bytes, 82)? != TARGET_LINUX
        || get_u16(bytes, 84)? != TARGET_X86_64
        || bytes[86..96].iter().any(|byte| *byte != 0)
    {
        return Err(ExecutableV2Error::UnsupportedMetadata);
    }
    Ok(Trailer {
        image_offset: get_u64(bytes, 16)?,
        image_length: get_u64(bytes, 24)?,
        executable_length: get_u64(bytes, 32)?,
        loader_length: get_u64(bytes, 40)?,
        loader_digest: array_32(bytes, 48)?,
    })
}

fn require_version_and_length(
    bytes: &[u8],
    required_length: usize,
) -> Result<(), ExecutableV2Error> {
    let version = get_u16(bytes, 8)?;
    if version != EXECUTABLE_V2_VERSION {
        return Err(ExecutableV2Error::UnsupportedVersion(version));
    }
    let length = get_u16(bytes, 10)?;
    if usize::from(length) != required_length {
        return Err(ExecutableV2Error::InvalidStructureLength(length));
    }
    Ok(())
}

fn build_info(
    header: &ImageHeader,
    trailer: &Trailer,
    manifest: ManifestV0,
    verification: Verification,
) -> ExecutableV2Info {
    ExecutableV2Info {
        schema_version: 1,
        executable_version: EXECUTABLE_V2_VERSION,
        runtime_abi_version: RUNTIME_V2_ABI_VERSION,
        verification,
        loader_size: trailer.loader_length,
        manifest_size: header.manifest_length,
        payload_size: header.payload_length,
        original_size: header.original_length,
        executable_size: trailer.executable_length,
        original_mode: header.original_mode,
        trailing_bytes: header.trailing_bytes,
        loader_digest: hex(&trailer.loader_digest),
        manifest_digest: hex(&header.manifest_digest),
        payload_digest: hex(&header.payload_digest),
        original_digest: hex(&header.original_digest),
        manifest,
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

fn usize_to_u64(value: usize, field: &'static str) -> Result<u64, ExecutableV2Error> {
    u64::try_from(value).map_err(|_| ExecutableV2Error::SizeOverflow(field))
}

fn to_usize(value: u64, field: &'static str) -> Result<usize, ExecutableV2Error> {
    usize::try_from(value).map_err(|_| ExecutableV2Error::SizeOverflow(field))
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

fn get_u16(input: &[u8], offset: usize) -> Result<u16, ExecutableV2Error> {
    let bytes = input
        .get(offset..offset + 2)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn get_u32(input: &[u8], offset: usize) -> Result<u32, ExecutableV2Error> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn get_u64(input: &[u8], offset: usize) -> Result<u64, ExecutableV2Error> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or(ExecutableV2Error::InvalidRange)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn array_32(input: &[u8], offset: usize) -> Result<[u8; 32], ExecutableV2Error> {
    input
        .get(offset..offset + 32)
        .ok_or(ExecutableV2Error::InvalidRange)?
        .try_into()
        .map_err(|_| ExecutableV2Error::InvalidRange)
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

#[cfg(test)]
mod tests {
    use super::{
        EXECUTABLE_V2_HEADER_LEN, ExecutableV2Error, PackOptions, Profile, Verification,
        inspect_executable_v2, pack_executable_v2, pack_executable_v2_with_codec,
        unpack_executable_v2, validate_direct_output_layout, verify_executable_v2,
    };
    use crate::executable_v2_codec4::CODEC_LZMA1_BCJ4;
    use crate::{LINUX_X86_64_RUNTIME_V2, MAX_ORIGINAL_SIZE};
    use crate::{ManifestSegment, ManifestV0};

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
        bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x40_0000u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&16_384u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&16_384u64.to_le_bytes());
        bytes[112..120].copy_from_slice(&4_096u64.to_le_bytes());
        bytes[512..].fill(0x41);
        bytes
    }

    fn packed() -> super::PackedExecutableV2 {
        pack_executable_v2(
            &fixture(),
            0o755,
            PackOptions {
                profile: Profile::Balanced,
                allow_larger: true,
            },
            LINUX_X86_64_RUNTIME_V2,
        )
        .unwrap()
    }

    #[test]
    fn deterministic_inspect_verify_and_unpack_round_trip() {
        let first = packed();
        let second = packed();
        assert_eq!(first.bytes, second.bytes);
        let inspected = inspect_executable_v2(&first.bytes).unwrap();
        assert_eq!(inspected.verification, Verification::Payload);
        assert_eq!(inspected.executable_version, 2);
        assert_eq!(inspected.manifest.segments.len(), 1);
        assert_eq!(
            verify_executable_v2(&first.bytes).unwrap().verification,
            Verification::Full
        );
        let unpacked = unpack_executable_v2(&first.bytes).unwrap();
        assert_eq!(unpacked.bytes, fixture());
        assert_eq!(unpacked.info.original_mode, 0o755);
    }

    #[test]
    fn codec4_host_round_trip_uses_canonical_four_stream_table() {
        let original = fixture();
        let packed = pack_executable_v2_with_codec(
            &original,
            0o755,
            PackOptions {
                profile: Profile::Balanced,
                allow_larger: true,
            },
            LINUX_X86_64_RUNTIME_V2,
            CODEC_LZMA1_BCJ4,
        )
        .unwrap();
        let repeated = pack_executable_v2_with_codec(
            &original,
            0o755,
            PackOptions {
                profile: Profile::Balanced,
                allow_larger: true,
            },
            LINUX_X86_64_RUNTIME_V2,
            CODEC_LZMA1_BCJ4,
        )
        .unwrap();
        assert_eq!(packed.bytes, repeated.bytes);
        assert_eq!(
            &packed.bytes[LINUX_X86_64_RUNTIME_V2.len() + 12..LINUX_X86_64_RUNTIME_V2.len() + 14],
            &CODEC_LZMA1_BCJ4.to_le_bytes()
        );
        assert_eq!(
            verify_executable_v2(&packed.bytes).unwrap().trailing_bytes,
            0
        );
        assert_eq!(unpack_executable_v2(&packed.bytes).unwrap().bytes, original);
    }

    #[test]
    fn rejects_header_manifest_payload_and_trailer_corruption() {
        let packed = packed();
        let loader_length = LINUX_X86_64_RUNTIME_V2.len();

        let mut header = packed.bytes.clone();
        header[loader_length + 16] ^= 1;
        assert!(matches!(
            inspect_executable_v2(&header),
            Err(ExecutableV2Error::Integrity("header"))
        ));

        let mut manifest = packed.bytes.clone();
        manifest[loader_length + EXECUTABLE_V2_HEADER_LEN] ^= 1;
        assert!(matches!(
            inspect_executable_v2(&manifest),
            Err(ExecutableV2Error::Integrity("manifest"))
        ));

        let mut payload = packed.bytes.clone();
        let payload_offset = loader_length
            + EXECUTABLE_V2_HEADER_LEN
            + usize::try_from(packed.info.manifest_size).unwrap();
        payload[payload_offset] ^= 1;
        assert!(matches!(
            inspect_executable_v2(&payload),
            Err(ExecutableV2Error::Integrity("payload"))
        ));

        let mut trailer = packed.bytes;
        let trailer_offset = trailer.len() - 128;
        trailer[trailer_offset + 16] ^= 1;
        assert!(matches!(
            inspect_executable_v2(&trailer),
            Err(ExecutableV2Error::Integrity("trailer"))
        ));
    }

    #[test]
    fn accepts_page_shifted_segments_and_rejects_oversized_direct_output_spans() {
        let mut manifest = ManifestV0 {
            original_size: 0x3000,
            entry_point: 0x0040_0100,
            segments: vec![
                ManifestSegment {
                    file_offset: 0,
                    file_size: 0x1000,
                    virtual_address: 0x0040_0000,
                    memory_size: 0x1000,
                    alignment: 0x1000,
                    flags: 5,
                },
                ManifestSegment {
                    file_offset: 0x2000,
                    file_size: 0x1000,
                    virtual_address: 0x0040_2000,
                    memory_size: 0x1000,
                    alignment: 0x1000,
                    flags: 4,
                },
            ],
        };
        assert!(validate_direct_output_layout(&manifest).is_ok());
        manifest.segments[1].virtual_address = 0x0040_3000;
        assert!(validate_direct_output_layout(&manifest).is_ok());
        manifest.segments.push(ManifestSegment {
            file_offset: 0x3000,
            file_size: 0x1000,
            virtual_address: 0x0040_5000,
            memory_size: 0x1000,
            alignment: 0x1000,
            flags: 4,
        });
        assert!(matches!(
            validate_direct_output_layout(&manifest),
            Err(ExecutableV2Error::UnsupportedDirectOutputLayout)
        ));
        manifest.segments.pop();
        manifest.segments[1].file_offset = MAX_ORIGINAL_SIZE;
        manifest.segments[1].virtual_address = 0x0040_0000 + MAX_ORIGINAL_SIZE;
        assert!(matches!(
            validate_direct_output_layout(&manifest),
            Err(ExecutableV2Error::UnsupportedDirectOutputLayout)
        ));
    }
}
