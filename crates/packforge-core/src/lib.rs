//! Safe host-side operations for Packforge containers.

mod container;
mod format;

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub use container::{
    ArtifactInfo, CONTAINER_VERSION, Codec, ContainerError, MAX_CONTAINER_SIZE, MAX_ORIGINAL_SIZE,
    PackOptions, PackedArtifact, Profile, UnpackedArtifact, Verification, inspect, pack, unpack,
    verify,
};
pub use format::{
    Architecture, BinaryClass, BinaryFormat, BinaryInfo, BinaryType, Endianness, FormatError,
    classify,
};

/// The current implementation stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStage {
    /// Reversible container and verification operations are implemented.
    ReversibleContainer,
}

impl ProjectStage {
    /// Returns the stable command-line representation of the stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReversibleContainer => "reversible-container",
        }
    }
}

/// Host-side file operation error.
#[derive(Debug)]
pub enum Error {
    /// Container or executable validation failed.
    Container(ContainerError),
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
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => error.fmt(formatter),
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
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Container(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::NotRegularFile(_) | Self::OutputExists(_) => None,
        }
    }
}

impl From<ContainerError> for Error {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

/// Returns the current implementation stage.
#[must_use]
pub const fn project_stage() -> ProjectStage {
    ProjectStage::ReversibleContainer
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
    fn reports_reversible_container_stage() {
        assert_eq!(project_stage(), ProjectStage::ReversibleContainer);
        assert_eq!(project_stage().as_str(), "reversible-container");
    }
}
