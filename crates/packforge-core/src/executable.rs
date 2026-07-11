//! Self-contained executable wrapper encoding.

use std::fmt;

use serde::Serialize;

use crate::container::{
    ArtifactInfo, CONTAINER_VERSION, ContainerError, HEADER_LEN, MAX_CONTAINER_SIZE, PackOptions,
    Profile, UnpackedArtifact, Verification, inspect, pack, unpack, verify,
};
use crate::format::{FormatError, classify};

/// Current self-contained executable format version.
pub const EXECUTABLE_VERSION: u16 = 1;
/// Current loader/runtime ABI version.
pub const RUNTIME_ABI_VERSION: u16 = 1;
/// Fixed trailer length for executable format version 1.
pub const EXECUTABLE_TRAILER_LEN: usize = 128;
const EXECUTABLE_TRAILER_LEN_U16: u16 = 128;
/// Initial release-gate budget for the native loader prefix.
pub const MAX_RUNTIME_STUB_SIZE: u64 = 32 * 1024;
/// Maximum complete executable accepted by the host implementation.
pub const MAX_EXECUTABLE_SIZE: u64 =
    MAX_RUNTIME_STUB_SIZE + MAX_CONTAINER_SIZE + EXECUTABLE_TRAILER_LEN as u64;

const MAGIC: &[u8; 8] = b"PFGEXE01";
const TRAILER_HASH_OFFSET: usize = 96;
const TARGET_LINUX: u16 = 1;
const TARGET_X86_64: u16 = 62;

/// Reproducible Linux x86-64 runtime artifact embedded by the host packer.
pub const LINUX_X86_64_RUNTIME: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../runtime/artifacts/linux-x86_64/loader-v1"
));

/// Stable information returned for a self-contained executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutableInfo {
    /// JSON/report schema version.
    pub schema_version: u16,
    /// Executable wrapper format version.
    pub executable_version: u16,
    /// Native loader ABI version.
    pub runtime_abi_version: u16,
    /// Validation depth reached for the embedded container.
    pub verification: Verification,
    /// Native loader prefix length.
    pub loader_size: u64,
    /// Embedded PFG container offset.
    pub container_offset: u64,
    /// Embedded PFG container length.
    pub container_size: u64,
    /// Complete self-contained executable length.
    pub executable_size: u64,
    /// BLAKE3 digest of the exact native loader bytes.
    pub loader_digest: String,
    /// Embedded recovery-container metadata.
    pub container: ArtifactInfo,
}

/// Bytes and metadata produced by executable packing.
#[derive(Debug)]
pub struct PackedExecutable {
    /// Complete executable bytes.
    pub bytes: Vec<u8>,
    /// Executable wrapper and embedded-container metadata.
    pub info: ExecutableInfo,
}

/// Bytes and metadata produced by executable unpacking.
#[derive(Debug)]
pub struct UnpackedExecutable {
    /// Byte-identical original executable.
    pub bytes: Vec<u8>,
    /// Fully verified executable metadata.
    pub info: ExecutableInfo,
}

/// Errors produced by bounded executable-wrapper processing.
#[derive(Debug)]
pub enum ExecutableError {
    /// Embedded recovery-container processing failed.
    Container(ContainerError),
    /// The native loader is malformed or outside the stable compatibility tier.
    LoaderFormat(FormatError),
    /// The artifact is shorter than the fixed trailer.
    TruncatedTrailer,
    /// The executable trailer magic is not recognized.
    InvalidMagic,
    /// The executable wrapper version is unsupported.
    UnsupportedVersion(u16),
    /// The fixed trailer length is unsupported.
    InvalidTrailerLength(u16),
    /// The runtime ABI is unsupported.
    UnsupportedRuntimeAbi(u16),
    /// Flags or reserved fields are nonzero.
    NonzeroReserved,
    /// The declared target is unsupported.
    UnsupportedTarget { operating_system: u16, machine: u16 },
    /// The trailer checksum is invalid.
    TrailerIntegrity,
    /// The complete executable length is invalid.
    InvalidExecutableLength { declared: u64, actual: u64 },
    /// The loader length is zero or exceeds the runtime budget.
    InvalidLoaderLength { actual: u64, maximum: u64 },
    /// The embedded container range is inconsistent with the artifact layout.
    InvalidContainerRange,
    /// The native loader checksum is invalid.
    LoaderIntegrity,
    /// The selected compression profile has no runtime decoder yet.
    UnsupportedRuntimeProfile(Profile),
    /// The complete executable would not reduce artifact size.
    NotBeneficial { original: u64, executable: u64 },
    /// A length cannot be represented safely on the host.
    SizeOverflow(&'static str),
}

impl fmt::Display for ExecutableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => error.fmt(formatter),
            Self::LoaderFormat(error) => write!(formatter, "invalid runtime loader: {error}"),
            Self::TruncatedTrailer => formatter.write_str("truncated Packforge executable trailer"),
            Self::InvalidMagic => formatter.write_str("input is not a Packforge executable"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported Packforge executable version {version}"
                )
            }
            Self::InvalidTrailerLength(length) => {
                write!(formatter, "unsupported Packforge trailer length {length}")
            }
            Self::UnsupportedRuntimeAbi(version) => {
                write!(formatter, "unsupported Packforge runtime ABI {version}")
            }
            Self::NonzeroReserved => {
                formatter.write_str("executable uses unsupported flags or reserved bytes")
            }
            Self::UnsupportedTarget {
                operating_system,
                machine,
            } => write!(
                formatter,
                "unsupported executable target os={operating_system} machine={machine}"
            ),
            Self::TrailerIntegrity => formatter.write_str("executable trailer checksum mismatch"),
            Self::InvalidExecutableLength { declared, actual } => write!(
                formatter,
                "executable length mismatch: trailer declares {declared} bytes, file has {actual}"
            ),
            Self::InvalidLoaderLength { actual, maximum } => write!(
                formatter,
                "runtime loader is {actual} bytes; the executable-format limit is {maximum} bytes"
            ),
            Self::InvalidContainerRange => {
                formatter.write_str("embedded container range is invalid")
            }
            Self::LoaderIntegrity => formatter.write_str("runtime loader checksum mismatch"),
            Self::UnsupportedRuntimeProfile(profile) => write!(
                formatter,
                "profile {} is not available for self-contained executables; use fast",
                profile.as_str()
            ),
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

impl std::error::Error for ExecutableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Container(error) => Some(error),
            Self::LoaderFormat(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ContainerError> for ExecutableError {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

#[derive(Debug)]
struct Trailer {
    container_offset: u64,
    container_length: u64,
    executable_length: u64,
    loader_length: u64,
    loader_digest: [u8; 32],
}

#[derive(Debug)]
struct ParsedExecutable<'a> {
    container: &'a [u8],
    trailer: Trailer,
}

/// Wraps a supported executable in a deterministic native loader.
///
/// # Errors
///
/// Returns [`ExecutableError`] if the loader or input is unsupported, packing
/// fails, lengths exceed hard limits, or the complete artifact is not smaller
/// without an explicit opt-in.
pub fn pack_executable(
    original: &[u8],
    original_mode: u32,
    options: PackOptions,
    loader: &[u8],
) -> Result<PackedExecutable, ExecutableError> {
    if options.profile != Profile::Fast {
        return Err(ExecutableError::UnsupportedRuntimeProfile(options.profile));
    }
    validate_loader(loader)?;

    let container = pack(
        original,
        original_mode,
        PackOptions {
            profile: options.profile,
            allow_larger: true,
        },
    )?;
    let loader_length = usize_to_u64(loader.len(), "runtime loader")?;
    let container_length = usize_to_u64(container.bytes.len(), "embedded container")?;
    let executable_length = loader_length
        .checked_add(container_length)
        .and_then(|length| length.checked_add(EXECUTABLE_TRAILER_LEN as u64))
        .ok_or(ExecutableError::SizeOverflow("self-contained executable"))?;
    if executable_length > MAX_EXECUTABLE_SIZE {
        return Err(ExecutableError::InvalidExecutableLength {
            declared: executable_length,
            actual: MAX_EXECUTABLE_SIZE,
        });
    }
    let original_length = usize_to_u64(original.len(), "original executable")?;
    if !options.allow_larger && executable_length >= original_length {
        return Err(ExecutableError::NotBeneficial {
            original: original_length,
            executable: executable_length,
        });
    }

    let trailer = Trailer {
        container_offset: loader_length,
        container_length,
        executable_length,
        loader_length,
        loader_digest: *blake3::hash(loader).as_bytes(),
    };
    let mut bytes = Vec::with_capacity(
        usize::try_from(executable_length)
            .map_err(|_| ExecutableError::SizeOverflow("self-contained executable"))?,
    );
    bytes.extend_from_slice(loader);
    bytes.extend_from_slice(&container.bytes);
    bytes.extend_from_slice(&encode_trailer(&trailer));

    Ok(PackedExecutable {
        info: executable_info(&trailer, container.info, Verification::Full),
        bytes,
    })
}

/// Inspects the wrapper, loader, and embedded compressed payload without
/// decompressing the original executable.
///
/// # Errors
///
/// Returns [`ExecutableError`] when any framing, target, range, loader, or
/// embedded-container integrity check fails.
pub fn inspect_executable(executable: &[u8]) -> Result<ExecutableInfo, ExecutableError> {
    let parsed = parse(executable)?;
    let container = inspect(parsed.container)?;
    Ok(executable_info(
        &parsed.trailer,
        container,
        Verification::Payload,
    ))
}

/// Fully verifies a self-contained executable without executing its payload.
///
/// # Errors
///
/// Returns [`ExecutableError`] when wrapper, compressed payload, decompression,
/// original digest, or executable classification validation fails.
pub fn verify_executable(executable: &[u8]) -> Result<ExecutableInfo, ExecutableError> {
    let parsed = parse(executable)?;
    let container = verify(parsed.container)?;
    Ok(executable_info(
        &parsed.trailer,
        container,
        Verification::Full,
    ))
}

/// Recovers the byte-identical original executable from a fully verified wrapper.
///
/// # Errors
///
/// Returns [`ExecutableError`] when wrapper or embedded-container validation
/// fails.
pub fn unpack_executable(executable: &[u8]) -> Result<UnpackedExecutable, ExecutableError> {
    let parsed = parse(executable)?;
    let UnpackedArtifact { bytes, info } = unpack(parsed.container)?;
    Ok(UnpackedExecutable {
        bytes,
        info: executable_info(&parsed.trailer, info, Verification::Full),
    })
}

fn parse(executable: &[u8]) -> Result<ParsedExecutable<'_>, ExecutableError> {
    let actual_length = usize_to_u64(executable.len(), "self-contained executable")?;
    if actual_length > MAX_EXECUTABLE_SIZE {
        return Err(ExecutableError::InvalidExecutableLength {
            declared: actual_length,
            actual: actual_length,
        });
    }
    let trailer_start = executable
        .len()
        .checked_sub(EXECUTABLE_TRAILER_LEN)
        .ok_or(ExecutableError::TruncatedTrailer)?;
    let trailer = decode_trailer(&executable[trailer_start..])?;
    if trailer.executable_length != actual_length {
        return Err(ExecutableError::InvalidExecutableLength {
            declared: trailer.executable_length,
            actual: actual_length,
        });
    }
    validate_loader_length(trailer.loader_length)?;
    if trailer.container_offset != trailer.loader_length {
        return Err(ExecutableError::InvalidContainerRange);
    }
    if trailer.container_length < HEADER_LEN as u64 || trailer.container_length > MAX_CONTAINER_SIZE
    {
        return Err(ExecutableError::InvalidContainerRange);
    }
    let container_end = trailer
        .container_offset
        .checked_add(trailer.container_length)
        .ok_or(ExecutableError::InvalidContainerRange)?;
    let expected_trailer_start = usize_to_u64(trailer_start, "trailer offset")?;
    if container_end != expected_trailer_start {
        return Err(ExecutableError::InvalidContainerRange);
    }
    let loader_end = usize::try_from(trailer.loader_length)
        .map_err(|_| ExecutableError::SizeOverflow("runtime loader"))?;
    let container_end = usize::try_from(container_end)
        .map_err(|_| ExecutableError::SizeOverflow("embedded container"))?;
    let loader = executable
        .get(..loader_end)
        .ok_or(ExecutableError::InvalidContainerRange)?;
    let container = executable
        .get(loader_end..container_end)
        .ok_or(ExecutableError::InvalidContainerRange)?;
    if blake3::hash(loader).as_bytes() != &trailer.loader_digest {
        return Err(ExecutableError::LoaderIntegrity);
    }
    validate_loader(loader)?;
    Ok(ParsedExecutable { container, trailer })
}

fn validate_loader(loader: &[u8]) -> Result<(), ExecutableError> {
    validate_loader_length(usize_to_u64(loader.len(), "runtime loader")?)?;
    classify(loader).map_err(ExecutableError::LoaderFormat)?;
    Ok(())
}

fn validate_loader_length(length: u64) -> Result<(), ExecutableError> {
    if length == 0 || length > MAX_RUNTIME_STUB_SIZE {
        return Err(ExecutableError::InvalidLoaderLength {
            actual: length,
            maximum: MAX_RUNTIME_STUB_SIZE,
        });
    }
    Ok(())
}

fn executable_info(
    trailer: &Trailer,
    container: ArtifactInfo,
    verification: Verification,
) -> ExecutableInfo {
    ExecutableInfo {
        schema_version: 2,
        executable_version: EXECUTABLE_VERSION,
        runtime_abi_version: RUNTIME_ABI_VERSION,
        verification,
        loader_size: trailer.loader_length,
        container_offset: trailer.container_offset,
        container_size: trailer.container_length,
        executable_size: trailer.executable_length,
        loader_digest: hex(&trailer.loader_digest),
        container,
    }
}

fn encode_trailer(trailer: &Trailer) -> [u8; EXECUTABLE_TRAILER_LEN] {
    let mut bytes = [0u8; EXECUTABLE_TRAILER_LEN];
    bytes[..8].copy_from_slice(MAGIC);
    put_u16(&mut bytes, 8, EXECUTABLE_VERSION);
    put_u16(&mut bytes, 10, EXECUTABLE_TRAILER_LEN_U16);
    put_u16(&mut bytes, 12, RUNTIME_ABI_VERSION);
    put_u64(&mut bytes, 16, trailer.container_offset);
    put_u64(&mut bytes, 24, trailer.container_length);
    put_u64(&mut bytes, 32, trailer.executable_length);
    put_u64(&mut bytes, 40, trailer.loader_length);
    bytes[48..80].copy_from_slice(&trailer.loader_digest);
    put_u16(&mut bytes, 80, CONTAINER_VERSION);
    put_u16(&mut bytes, 82, TARGET_LINUX);
    put_u16(&mut bytes, 84, TARGET_X86_64);
    let hash = blake3::hash(&bytes);
    bytes[TRAILER_HASH_OFFSET..].copy_from_slice(hash.as_bytes());
    bytes
}

fn decode_trailer(bytes: &[u8]) -> Result<Trailer, ExecutableError> {
    if bytes.len() != EXECUTABLE_TRAILER_LEN {
        return Err(ExecutableError::TruncatedTrailer);
    }
    if bytes.get(..8) != Some(MAGIC) {
        return Err(ExecutableError::InvalidMagic);
    }
    let version = get_u16(bytes, 8)?;
    if version != EXECUTABLE_VERSION {
        return Err(ExecutableError::UnsupportedVersion(version));
    }
    let trailer_length = get_u16(bytes, 10)?;
    if usize::from(trailer_length) != EXECUTABLE_TRAILER_LEN {
        return Err(ExecutableError::InvalidTrailerLength(trailer_length));
    }
    let stored_hash = bytes
        .get(TRAILER_HASH_OFFSET..)
        .ok_or(ExecutableError::TruncatedTrailer)?;
    let mut hash_input = [0u8; EXECUTABLE_TRAILER_LEN];
    hash_input.copy_from_slice(bytes);
    hash_input[TRAILER_HASH_OFFSET..].fill(0);
    if blake3::hash(&hash_input).as_bytes().as_slice() != stored_hash {
        return Err(ExecutableError::TrailerIntegrity);
    }
    let runtime_abi = get_u16(bytes, 12)?;
    if runtime_abi != RUNTIME_ABI_VERSION {
        return Err(ExecutableError::UnsupportedRuntimeAbi(runtime_abi));
    }
    if get_u16(bytes, 14)? != 0 || bytes[86..96].iter().any(|byte| *byte != 0) {
        return Err(ExecutableError::NonzeroReserved);
    }
    let container_version = get_u16(bytes, 80)?;
    if container_version != CONTAINER_VERSION {
        return Err(ExecutableError::Container(
            ContainerError::UnsupportedVersion(container_version),
        ));
    }
    let operating_system = get_u16(bytes, 82)?;
    let machine = get_u16(bytes, 84)?;
    if operating_system != TARGET_LINUX || machine != TARGET_X86_64 {
        return Err(ExecutableError::UnsupportedTarget {
            operating_system,
            machine,
        });
    }
    Ok(Trailer {
        container_offset: get_u64(bytes, 16)?,
        container_length: get_u64(bytes, 24)?,
        executable_length: get_u64(bytes, 32)?,
        loader_length: get_u64(bytes, 40)?,
        loader_digest: get_array_32(bytes, 48)?,
    })
}

fn usize_to_u64(value: usize, field: &'static str) -> Result<u64, ExecutableError> {
    u64::try_from(value).map_err(|_| ExecutableError::SizeOverflow(field))
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

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(input: &[u8], offset: usize) -> Result<u16, ExecutableError> {
    let bytes = input
        .get(offset..offset + 2)
        .ok_or(ExecutableError::TruncatedTrailer)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn get_u64(input: &[u8], offset: usize) -> Result<u64, ExecutableError> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or(ExecutableError::TruncatedTrailer)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn get_array_32(input: &[u8], offset: usize) -> Result<[u8; 32], ExecutableError> {
    input
        .get(offset..offset + 32)
        .ok_or(ExecutableError::TruncatedTrailer)?
        .try_into()
        .map_err(|_| ExecutableError::TruncatedTrailer)
}

#[cfg(test)]
mod tests {
    use super::{
        EXECUTABLE_TRAILER_LEN, ExecutableError, PackOptions, Profile, Verification,
        encode_trailer, inspect_executable, pack_executable, unpack_executable, verify_executable,
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

    fn options() -> PackOptions {
        PackOptions {
            profile: Profile::Fast,
            allow_larger: true,
        }
    }

    #[test]
    fn executable_round_trips_and_reports_validation_depth() {
        let original = fixture();
        let packed = pack_executable(&original, 0o755, options(), &fixture()).unwrap();
        assert_eq!(
            inspect_executable(&packed.bytes).unwrap().verification,
            Verification::Payload
        );
        assert_eq!(
            verify_executable(&packed.bytes).unwrap().verification,
            Verification::Full
        );
        let unpacked = unpack_executable(&packed.bytes).unwrap();
        assert_eq!(unpacked.bytes, original);
        assert_eq!(unpacked.info.container.original_mode, 0o755);
    }

    #[test]
    fn executable_output_is_deterministic() {
        let original = fixture();
        let loader = fixture();
        let first = pack_executable(&original, 0o755, options(), &loader).unwrap();
        let second = pack_executable(&original, 0o755, options(), &loader).unwrap();
        assert_eq!(first.bytes, second.bytes);
    }

    #[test]
    fn trailer_corruption_is_detected_before_offsets_are_used() {
        let mut packed = pack_executable(&fixture(), 0o755, options(), &fixture())
            .unwrap()
            .bytes;
        let offset = packed.len() - EXECUTABLE_TRAILER_LEN + 24;
        packed[offset] ^= 0x80;
        assert!(matches!(
            inspect_executable(&packed),
            Err(ExecutableError::TrailerIntegrity)
        ));
    }

    #[test]
    fn loader_corruption_is_detected() {
        let mut packed = pack_executable(&fixture(), 0o755, options(), &fixture())
            .unwrap()
            .bytes;
        packed[256] ^= 0x80;
        assert!(matches!(
            inspect_executable(&packed),
            Err(ExecutableError::LoaderIntegrity)
        ));
    }

    #[test]
    fn embedded_container_corruption_is_detected() {
        let loader = fixture();
        let mut packed = pack_executable(&fixture(), 0o755, options(), &loader)
            .unwrap()
            .bytes;
        packed[loader.len() + 192] ^= 0x80;
        assert!(matches!(
            inspect_executable(&packed),
            Err(ExecutableError::Container(_))
        ));
    }

    #[test]
    fn integrity_checked_length_mismatch_is_rejected() {
        let mut packed = pack_executable(&fixture(), 0o755, options(), &fixture())
            .unwrap()
            .bytes;
        let trailer_start = packed.len() - EXECUTABLE_TRAILER_LEN;
        let mut trailer = super::decode_trailer(&packed[trailer_start..]).unwrap();
        trailer.executable_length += 1;
        packed[trailer_start..].copy_from_slice(&encode_trailer(&trailer));
        assert!(matches!(
            inspect_executable(&packed),
            Err(ExecutableError::InvalidExecutableLength { .. })
        ));
    }

    #[test]
    fn unavailable_runtime_profile_is_rejected() {
        let result = pack_executable(
            &fixture(),
            0o755,
            PackOptions {
                profile: Profile::Balanced,
                allow_larger: true,
            },
            &fixture(),
        );
        assert!(matches!(
            result,
            Err(ExecutableError::UnsupportedRuntimeProfile(
                Profile::Balanced
            ))
        ));
    }

    #[test]
    fn complete_executable_overhead_requires_opt_in() {
        let result = pack_executable(
            &fixture(),
            0o755,
            PackOptions {
                profile: Profile::Fast,
                allow_larger: false,
            },
            &fixture(),
        );
        assert!(matches!(result, Err(ExecutableError::NotBeneficial { .. })));
    }

    #[test]
    fn truncated_input_is_rejected() {
        assert!(matches!(
            inspect_executable(&[0u8; EXECUTABLE_TRAILER_LEN - 1]),
            Err(ExecutableError::TruncatedTrailer)
        ));
    }
}
