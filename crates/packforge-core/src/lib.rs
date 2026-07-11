//! Safe host-side operations for Packforge containers.

#![forbid(unsafe_code)]

mod benchmark;
mod container;
mod diagnostic;
mod executable;
mod format;
mod manifest;

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub use benchmark::{BenchmarkReport, MAX_BENCHMARK_ITERATIONS, ProfileBenchmark, benchmark};
pub use container::{
    ArtifactInfo, CONTAINER_VERSION, Codec, ContainerError, MAX_CONTAINER_SIZE, MAX_ORIGINAL_SIZE,
    PackOptions, PackedArtifact, Profile, UnpackedArtifact, Verification, inspect, pack, unpack,
    verify,
};
pub use diagnostic::{Diagnostic, DiagnosticClass, INTERNAL_DIAGNOSTIC};
pub use executable::{
    EXECUTABLE_TRAILER_LEN, EXECUTABLE_VERSION, ExecutableError, ExecutableInfo,
    LINUX_X86_64_RUNTIME, MAX_EXECUTABLE_SIZE, MAX_RUNTIME_STUB_SIZE, PackedExecutable,
    RUNTIME_ABI_VERSION, UnpackedExecutable, inspect_executable, pack_executable,
    unpack_executable, verify_executable,
};
pub use format::{
    Architecture, BinaryClass, BinaryFormat, BinaryInfo, BinaryType, Endianness, FormatError,
    classify,
};
pub use manifest::{
    MANIFEST_HEADER_LEN, MANIFEST_SEGMENT_LEN, MANIFEST_VERSION, MAX_MANIFEST_MEMORY_SIZE,
    MAX_MANIFEST_SEGMENTS, ManifestError, ManifestSegment, ManifestV0, decode_manifest_v0,
};

/// The current implementation stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStage {
    /// Reversible container and verification operations are implemented.
    ReversibleContainer,
    /// A native self-contained executable runtime is under compatibility testing.
    RuntimeSpike,
}

impl ProjectStage {
    /// Returns the stable command-line representation of the stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReversibleContainer => "reversible-container",
            Self::RuntimeSpike => "runtime-spike",
        }
    }
}

/// Host-side file operation error.
#[derive(Debug)]
pub enum Error {
    /// Container or executable validation failed.
    Container(ContainerError),
    /// Self-contained executable validation failed.
    Executable(ExecutableError),
    /// A filesystem operation failed.
    Io {
        /// Operation being attempted.
        operation: &'static str,
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The input path is not a regular file.
    NotRegularFile(PathBuf),
    /// The output exists; Packforge never clobbers by default.
    OutputExists(PathBuf),
    /// An artifact is neither a recovery container nor a packed executable.
    UnknownArtifact,
    /// Native output requires an executable input file mode.
    InputNotExecutable(PathBuf),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => error.fmt(formatter),
            Self::Executable(error) => error.fmt(formatter),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "could not {operation} {}: {source}",
                path.display()
            ),
            Self::NotRegularFile(path) => {
                write!(formatter, "input is not a regular file: {}", path.display())
            }
            Self::OutputExists(path) => write!(
                formatter,
                "refusing to overwrite existing output {}; choose another path",
                path.display()
            ),
            Self::UnknownArtifact => formatter.write_str("input is not a Packforge artifact"),
            Self::InputNotExecutable(path) => write!(
                formatter,
                "input is not executable: {}; set an execute permission before packing native output",
                path.display()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Container(error) => Some(error),
            Self::Executable(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::NotRegularFile(_)
            | Self::OutputExists(_)
            | Self::UnknownArtifact
            | Self::InputNotExecutable(_) => None,
        }
    }
}

impl From<ContainerError> for Error {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

impl From<ExecutableError> for Error {
    fn from(error: ExecutableError) -> Self {
        Self::Executable(error)
    }
}

/// Metadata returned by auto-detecting artifact operations.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "artifact_kind", rename_all = "snake_case")]
pub enum ArtifactReport {
    /// A standalone reversible PFG container.
    Container {
        /// Existing container report fields remain at the JSON top level.
        #[serde(flatten)]
        info: ArtifactInfo,
    },
    /// A native self-contained executable.
    Executable {
        /// Executable wrapper and embedded-container report fields.
        #[serde(flatten)]
        info: ExecutableInfo,
    },
}

/// Returns the current implementation stage.
#[must_use]
pub const fn project_stage() -> ProjectStage {
    ProjectStage::RuntimeSpike
}

/// Packs a supported executable and atomically creates a new container file.
///
/// # Errors
///
/// Returns [`Error`] when input I/O, executable validation, compression, or atomic
/// no-clobber output creation fails.
pub fn pack_file(
    input_path: &Path,
    output_path: &Path,
    options: PackOptions,
) -> Result<ArtifactInfo, Error> {
    let (input, metadata) = read_bounded(input_path, MAX_ORIGINAL_SIZE)?;
    let original_mode = file_mode(&metadata);
    let artifact = pack(&input, original_mode, options)?;
    write_new(output_path, &artifact.bytes, 0o644)?;
    Ok(artifact.info)
}

/// Packs a supported executable into a native self-contained Linux artifact.
///
/// # Errors
///
/// Returns [`Error`] when input I/O, executable permissions, validation,
/// compression, runtime wrapping, or atomic output creation fails.
pub fn pack_executable_file(
    input_path: &Path,
    output_path: &Path,
    options: PackOptions,
) -> Result<ExecutableInfo, Error> {
    let (input, metadata) = read_bounded(input_path, MAX_ORIGINAL_SIZE)?;
    let original_mode = file_mode(&metadata);
    if original_mode & 0o111 == 0 {
        return Err(Error::InputNotExecutable(input_path.to_path_buf()));
    }
    let artifact = pack_executable(&input, original_mode, options, LINUX_X86_64_RUNTIME)?;
    write_new(output_path, &artifact.bytes, original_mode & 0o777)?;
    Ok(artifact.info)
}

/// Inspects either a recovery container or a self-contained executable.
///
/// # Errors
///
/// Returns [`Error`] when the input cannot be read, its artifact kind is not
/// recognized, or framing and compressed-payload validation fails.
pub fn inspect_artifact_file(input_path: &Path) -> Result<ArtifactReport, Error> {
    let (input, _) = read_bounded(input_path, MAX_EXECUTABLE_SIZE)?;
    match detect_artifact(&input)? {
        ArtifactKind::Container => Ok(ArtifactReport::Container {
            info: inspect(&input)?,
        }),
        ArtifactKind::Executable => Ok(ArtifactReport::Executable {
            info: inspect_executable(&input)?,
        }),
    }
}

/// Fully verifies either supported artifact kind without executing it.
///
/// # Errors
///
/// Returns [`Error`] when artifact detection, wrapper/container validation,
/// decompression, digest validation, or metadata reclassification fails.
pub fn verify_artifact_file(input_path: &Path) -> Result<ArtifactReport, Error> {
    let (input, _) = read_bounded(input_path, MAX_EXECUTABLE_SIZE)?;
    match detect_artifact(&input)? {
        ArtifactKind::Container => Ok(ArtifactReport::Container {
            info: verify(&input)?,
        }),
        ArtifactKind::Executable => Ok(ArtifactReport::Executable {
            info: verify_executable(&input)?,
        }),
    }
}

/// Recovers the original executable from either supported artifact kind.
///
/// # Errors
///
/// Returns [`Error`] when verification fails or the no-clobber output cannot be
/// created and synchronized.
pub fn unpack_artifact_file(
    input_path: &Path,
    output_path: &Path,
) -> Result<ArtifactReport, Error> {
    let (input, _) = read_bounded(input_path, MAX_EXECUTABLE_SIZE)?;
    match detect_artifact(&input)? {
        ArtifactKind::Container => {
            let artifact = unpack(&input)?;
            let mode = recovered_mode(artifact.info.original_mode);
            write_new(output_path, &artifact.bytes, mode)?;
            Ok(ArtifactReport::Container {
                info: artifact.info,
            })
        }
        ArtifactKind::Executable => {
            let artifact = unpack_executable(&input)?;
            let mode = recovered_mode(artifact.info.container.original_mode);
            write_new(output_path, &artifact.bytes, mode)?;
            Ok(ArtifactReport::Executable {
                info: artifact.info,
            })
        }
    }
}

/// Inspects a container without decompressing its executable payload.
///
/// # Errors
///
/// Returns [`Error`] when the input cannot be read or its container framing,
/// metadata, limits, or compressed payload integrity are invalid.
pub fn inspect_file(input_path: &Path) -> Result<ArtifactInfo, Error> {
    let (input, _) = read_bounded(input_path, MAX_CONTAINER_SIZE)?;
    inspect(&input).map_err(Error::from)
}

/// Fully verifies a container without writing its executable payload.
///
/// # Errors
///
/// Returns [`Error`] when input I/O, container validation, decompression,
/// executable integrity, or metadata reclassification fails.
pub fn verify_file(input_path: &Path) -> Result<ArtifactInfo, Error> {
    let (input, _) = read_bounded(input_path, MAX_CONTAINER_SIZE)?;
    verify(&input).map_err(Error::from)
}

/// Fully verifies a container and atomically creates the original executable.
///
/// # Errors
///
/// Returns [`Error`] when verification fails or the no-clobber output cannot be
/// created and synchronized.
pub fn unpack_file(input_path: &Path, output_path: &Path) -> Result<ArtifactInfo, Error> {
    let (input, _) = read_bounded(input_path, MAX_CONTAINER_SIZE)?;
    let artifact = unpack(&input)?;
    let mode = if artifact.info.original_mode == 0 {
        0o755
    } else {
        artifact.info.original_mode
    };
    write_new(output_path, &artifact.bytes, mode)?;
    Ok(artifact.info)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
    Container,
    Executable,
}

fn detect_artifact(input: &[u8]) -> Result<ArtifactKind, Error> {
    if input.get(..8) == Some(b"PFGCNT01") {
        return Ok(ArtifactKind::Container);
    }
    if input
        .len()
        .checked_sub(EXECUTABLE_TRAILER_LEN)
        .and_then(|offset| input.get(offset..offset + 8))
        == Some(b"PFGEXE01")
    {
        return Ok(ArtifactKind::Executable);
    }
    Err(Error::UnknownArtifact)
}

const fn recovered_mode(original_mode: u32) -> u32 {
    if original_mode == 0 {
        0o755
    } else {
        original_mode
    }
}

/// Benchmarks every stable profile without creating an output file.
///
/// # Errors
///
/// Returns [`Error`] when input I/O, executable validation, compression,
/// verification, determinism, or iteration-limit checks fail.
pub fn benchmark_file(input_path: &Path, iterations: u32) -> Result<BenchmarkReport, Error> {
    let (input, metadata) = read_bounded(input_path, MAX_ORIGINAL_SIZE)?;
    benchmark(&input, file_mode(&metadata), iterations).map_err(Error::from)
}

fn read_bounded(path: &Path, maximum: u64) -> Result<(Vec<u8>, fs::Metadata), Error> {
    let metadata = fs::metadata(path).map_err(|source| Error::Io {
        operation: "stat",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(Error::NotRegularFile(path.to_path_buf()));
    }
    if metadata.len() == 0 || metadata.len() > maximum {
        return Err(ContainerError::SizeLimit {
            field: "input file",
            actual: metadata.len(),
            maximum,
        }
        .into());
    }
    let bytes = fs::read(path).map_err(|source| Error::Io {
        operation: "read",
        path: path.to_path_buf(),
        source,
    })?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual == 0 || actual > maximum {
        return Err(ContainerError::SizeLimit {
            field: "input file",
            actual,
            maximum,
        }
        .into());
    }
    Ok((bytes, metadata))
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|source| Error::Io {
        operation: "create temporary output in",
        path: parent.to_path_buf(),
        source,
    })?;
    temporary.write_all(bytes).map_err(|source| Error::Io {
        operation: "write temporary output for",
        path: path.to_path_buf(),
        source,
    })?;
    temporary.flush().map_err(|source| Error::Io {
        operation: "flush temporary output for",
        path: path.to_path_buf(),
        source,
    })?;
    temporary.as_file().sync_all().map_err(|source| Error::Io {
        operation: "synchronize temporary output for",
        path: path.to_path_buf(),
        source,
    })?;
    set_file_mode(temporary.as_file(), mode, path)?;
    temporary.persist_noclobber(path).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::OutputExists(path.to_path_buf())
        } else {
            Error::Io {
                operation: "persist output",
                path: path.to_path_buf(),
                source: error.error,
            }
        }
    })?;
    Ok(())
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn set_file_mode(file: &fs::File, mode: u32, path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))
        .map_err(|source| Error::Io {
            operation: "set permissions on output",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn set_file_mode(_file: &fs::File, _mode: u32, _path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ProjectStage, project_stage};

    #[test]
    fn reports_runtime_spike_stage() {
        assert_eq!(project_stage(), ProjectStage::RuntimeSpike);
        assert_eq!(project_stage().as_str(), "runtime-spike");
    }
}
