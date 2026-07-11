//! Bounded executable-format classification.

use std::fmt;

use serde::Serialize;

const ELF64_HEADER_LEN: usize = 64;
const ELF64_HEADER_LEN_U16: u16 = 64;
const ELF64_PROGRAM_HEADER_LEN: usize = 56;
const ELF64_PROGRAM_HEADER_LEN_U16: u16 = 56;
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_LITTLE_ENDIAN: u8 = 1;
const ELF_VERSION_CURRENT: u8 = 1;
const ELF_TYPE_EXECUTABLE: u16 = 2;
const ELF_MACHINE_X86_64: u16 = 62;
const PROGRAM_TYPE_DYNAMIC: u32 = 2;
const PROGRAM_TYPE_INTERPRETER: u32 = 3;
const PROGRAM_TYPE_LOAD: u32 = 1;

/// The executable format recognized by the current compatibility tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryFormat {
    /// Executable and Linkable Format.
    Elf,
}

/// The ELF class recognized by the current compatibility tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryClass {
    /// A 64-bit object.
    Elf64,
}

/// The byte order recognized by the current compatibility tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Endianness {
    /// Least-significant byte first.
    Little,
}

/// The architecture recognized by the current compatibility tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    /// AMD64 / Intel 64.
    X86_64,
}

/// The executable type recognized by the current compatibility tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryType {
    /// A statically linked, non-PIE executable.
    StaticExecutable,
}

/// Format facts embedded in a Packforge container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BinaryInfo {
    /// Source executable format.
    pub format: BinaryFormat,
    /// Source executable class.
    pub class: BinaryClass,
    /// Source byte order.
    pub endianness: Endianness,
    /// Source architecture.
    pub architecture: Architecture,
    /// Supported compatibility tier.
    pub binary_type: BinaryType,
    /// Raw ELF machine identifier.
    pub machine: u16,
    /// Raw ELF file type.
    pub file_type: u16,
    /// Original entry point.
    pub entry_point: u64,
    /// Number of loadable segments.
    pub load_segments: u16,
}

/// Why an input is not in the current stable compatibility tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The file is shorter than the required structure.
    Truncated(&'static str),
    /// The file does not have the ELF magic bytes.
    NotElf,
    /// The ELF identification version is not current.
    UnsupportedIdentificationVersion(u8),
    /// The ELF class is not 64-bit.
    UnsupportedClass(u8),
    /// The ELF byte order is not little-endian.
    UnsupportedEndianness(u8),
    /// The ELF header version is not current.
    UnsupportedHeaderVersion(u32),
    /// The ELF object type is not a static `ET_EXEC` candidate.
    UnsupportedFileType(u16),
    /// The ELF machine is not x86-64.
    UnsupportedMachine(u16),
    /// A fixed-size ELF structure has an unexpected size.
    InvalidStructureSize {
        /// Structure name.
        structure: &'static str,
        /// Required byte length.
        expected: u16,
        /// Observed byte length.
        actual: u16,
    },
    /// A program-header table range is invalid.
    InvalidProgramHeaderTable,
    /// A load segment has an invalid file range.
    InvalidLoadSegment(u16),
    /// The executable uses a feature outside the current tier.
    UnsupportedFeature(&'static str),
    /// No loadable segment was found.
    NoLoadSegments,
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(structure) => write!(formatter, "truncated {structure}"),
            Self::NotElf => formatter.write_str("input is not an ELF executable"),
            Self::UnsupportedIdentificationVersion(version) => {
                write!(
                    formatter,
                    "unsupported ELF identification version {version}"
                )
            }
            Self::UnsupportedClass(class) => {
                write!(formatter, "unsupported ELF class {class}; expected ELF64")
            }
            Self::UnsupportedEndianness(endianness) => write!(
                formatter,
                "unsupported ELF byte order {endianness}; expected little-endian"
            ),
            Self::UnsupportedHeaderVersion(version) => {
                write!(formatter, "unsupported ELF header version {version}")
            }
            Self::UnsupportedFileType(file_type) => write!(
                formatter,
                "unsupported ELF file type {file_type}; M1 accepts ET_EXEC only"
            ),
            Self::UnsupportedMachine(machine) => write!(
                formatter,
                "unsupported ELF machine {machine}; M1 accepts x86-64 only"
            ),
            Self::InvalidStructureSize {
                structure,
                expected,
                actual,
            } => write!(
                formatter,
                "invalid {structure} size {actual}; expected {expected}"
            ),
            Self::InvalidProgramHeaderTable => {
                formatter.write_str("ELF program-header table is out of bounds")
            }
            Self::InvalidLoadSegment(index) => {
                write!(
                    formatter,
                    "ELF load segment {index} has an invalid file range"
                )
            }
            Self::UnsupportedFeature(feature) => {
                write!(formatter, "unsupported ELF feature in M1: {feature}")
            }
            Self::NoLoadSegments => formatter.write_str("ELF executable has no loadable segments"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Classifies an input against the M1 ELF compatibility tier.
///
/// The parser validates every range it reads and rejects dynamic linking rather
/// than silently classifying an executable as static.
///
/// # Errors
///
/// Returns [`FormatError`] when the input is malformed or uses an executable
/// feature outside the current static ELF64 x86-64 tier.
pub fn classify(input: &[u8]) -> Result<BinaryInfo, FormatError> {
    let identification = input
        .get(..16)
        .ok_or(FormatError::Truncated("ELF identification"))?;
    if identification.get(..4) != Some(b"\x7fELF") {
        return Err(FormatError::NotElf);
    }
    if identification[4] != ELF_CLASS_64 {
        return Err(FormatError::UnsupportedClass(identification[4]));
    }
    if identification[5] != ELF_DATA_LITTLE_ENDIAN {
        return Err(FormatError::UnsupportedEndianness(identification[5]));
    }
    if identification[6] != ELF_VERSION_CURRENT {
        return Err(FormatError::UnsupportedIdentificationVersion(
            identification[6],
        ));
    }

    let header = input
        .get(..ELF64_HEADER_LEN)
        .ok_or(FormatError::Truncated("ELF64 header"))?;
    let file_type = read_u16(header, 16)?;
    if file_type != ELF_TYPE_EXECUTABLE {
        return Err(FormatError::UnsupportedFileType(file_type));
    }
    let machine = read_u16(header, 18)?;
    if machine != ELF_MACHINE_X86_64 {
        return Err(FormatError::UnsupportedMachine(machine));
    }
    let header_version = read_u32(header, 20)?;
    if header_version != u32::from(ELF_VERSION_CURRENT) {
        return Err(FormatError::UnsupportedHeaderVersion(header_version));
    }

    let entry_point = read_u64(header, 24)?;
    let program_header_offset = read_u64(header, 32)?;
    let elf_header_size = read_u16(header, 52)?;
    if usize::from(elf_header_size) != ELF64_HEADER_LEN {
        return Err(FormatError::InvalidStructureSize {
            structure: "ELF64 header",
            expected: ELF64_HEADER_LEN_U16,
            actual: elf_header_size,
        });
    }
    let program_header_size = read_u16(header, 54)?;
    if usize::from(program_header_size) != ELF64_PROGRAM_HEADER_LEN {
        return Err(FormatError::InvalidStructureSize {
            structure: "ELF64 program header",
            expected: ELF64_PROGRAM_HEADER_LEN_U16,
            actual: program_header_size,
        });
    }
    let program_header_count = read_u16(header, 56)?;

    let table_start = usize::try_from(program_header_offset)
        .map_err(|_| FormatError::InvalidProgramHeaderTable)?;
    let table_len = usize::from(program_header_size)
        .checked_mul(usize::from(program_header_count))
        .ok_or(FormatError::InvalidProgramHeaderTable)?;
    let table_end = table_start
        .checked_add(table_len)
        .ok_or(FormatError::InvalidProgramHeaderTable)?;
    if table_end > input.len() {
        return Err(FormatError::InvalidProgramHeaderTable);
    }

    let mut load_segments = 0u16;
    for index in 0..program_header_count {
        let offset = table_start + usize::from(index) * ELF64_PROGRAM_HEADER_LEN;
        let program_header = &input[offset..offset + ELF64_PROGRAM_HEADER_LEN];
        match read_u32(program_header, 0)? {
            PROGRAM_TYPE_INTERPRETER => {
                return Err(FormatError::UnsupportedFeature("PT_INTERP dynamic loader"));
            }
            PROGRAM_TYPE_DYNAMIC => {
                return Err(FormatError::UnsupportedFeature(
                    "PT_DYNAMIC linking metadata",
                ));
            }
            PROGRAM_TYPE_LOAD => {
                validate_load_segment(input.len(), program_header, index)?;
                load_segments = load_segments
                    .checked_add(1)
                    .ok_or(FormatError::InvalidProgramHeaderTable)?;
            }
            _ => {}
        }
    }

    if load_segments == 0 {
        return Err(FormatError::NoLoadSegments);
    }

    Ok(BinaryInfo {
        format: BinaryFormat::Elf,
        class: BinaryClass::Elf64,
        endianness: Endianness::Little,
        architecture: Architecture::X86_64,
        binary_type: BinaryType::StaticExecutable,
        machine,
        file_type,
        entry_point,
        load_segments,
    })
}

fn validate_load_segment(
    input_len: usize,
    program_header: &[u8],
    index: u16,
) -> Result<(), FormatError> {
    let file_offset = read_u64(program_header, 8)?;
    let file_size = read_u64(program_header, 32)?;
    let memory_size = read_u64(program_header, 40)?;
    if file_size > memory_size {
        return Err(FormatError::InvalidLoadSegment(index));
    }
    let segment_end = file_offset
        .checked_add(file_size)
        .ok_or(FormatError::InvalidLoadSegment(index))?;
    if segment_end > u64::try_from(input_len).unwrap_or(u64::MAX) {
        return Err(FormatError::InvalidLoadSegment(index));
    }
    Ok(())
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, FormatError> {
    let bytes = input
        .get(offset..offset + 2)
        .ok_or(FormatError::Truncated("ELF field"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, FormatError> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or(FormatError::Truncated("ELF field"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(input: &[u8], offset: usize) -> Result<u64, FormatError> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or(FormatError::Truncated("ELF field"))?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::{Architecture, BinaryType, FormatError, classify};

    fn fixture() -> Vec<u8> {
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
        bytes[72..80].copy_from_slice(&0u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&4_096u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&4_096u64.to_le_bytes());
        bytes[256..].fill(0x41);
        bytes
    }

    #[test]
    fn classifies_static_x86_64_elf() {
        let info = classify(&fixture()).expect("fixture should classify");
        assert_eq!(info.architecture, Architecture::X86_64);
        assert_eq!(info.binary_type, BinaryType::StaticExecutable);
        assert_eq!(info.load_segments, 1);
    }

    #[test]
    fn rejects_dynamic_loader_segment() {
        let mut bytes = fixture();
        bytes[64..68].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            classify(&bytes),
            Err(FormatError::UnsupportedFeature("PT_INTERP dynamic loader"))
        );
    }

    #[test]
    fn rejects_out_of_bounds_load_segment() {
        let mut bytes = fixture();
        bytes[72..80].copy_from_slice(&4_000u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&200u64.to_le_bytes());
        assert_eq!(classify(&bytes), Err(FormatError::InvalidLoadSegment(0)));
    }
}
