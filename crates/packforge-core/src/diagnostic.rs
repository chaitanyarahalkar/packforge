//! Stable error classes and diagnostic identifiers for automation.

use crate::{ContainerError, Error, ExecutableError, FormatError};

/// Broad failure class used to select a stable process exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticClass {
    /// The input or requested feature is outside the supported compatibility tier.
    Unsupported,
    /// An artifact is malformed, inconsistent, or fails an integrity check.
    Corrupt,
    /// A declared or requested resource exceeds a hard bound.
    ResourceLimit,
    /// A filesystem operation failed.
    Io,
    /// A safe no-clobber or policy decision refused the requested output.
    Conflict,
    /// Packforge itself failed an invariant or output operation.
    Internal,
}

impl DiagnosticClass {
    /// Stable lowercase name used by documentation and machine consumers.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Corrupt => "corrupt",
            Self::ResourceLimit => "resource_limit",
            Self::Io => "io",
            Self::Conflict => "conflict",
            Self::Internal => "internal",
        }
    }

    /// Stable process exit status for this class.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Unsupported => 3,
            Self::Corrupt => 4,
            Self::ResourceLimit => 5,
            Self::Io => 6,
            Self::Conflict => 7,
            Self::Internal => 70,
        }
    }
}

/// Stable identifier and class attached to a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    /// Versioned diagnostic identifier. Existing meanings are never reused.
    pub code: &'static str,
    /// Broad automation class.
    pub class: DiagnosticClass,
}

impl Diagnostic {
    /// Constructs a diagnostic mapping.
    #[must_use]
    pub const fn new(code: &'static str, class: DiagnosticClass) -> Self {
        Self { code, class }
    }

    /// Stable process exit status for this diagnostic.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        self.class.exit_code()
    }
}

/// Used only when serialization/output or an internal invariant fails.
pub const INTERNAL_DIAGNOSTIC: Diagnostic = Diagnostic::new("PFG5001", DiagnosticClass::Internal);

impl FormatError {
    /// Returns the stable diagnostic mapping for an ELF classification error.
    #[must_use]
    pub const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::NotElf
            | Self::UnsupportedIdentificationVersion(_)
            | Self::UnsupportedClass(_)
            | Self::UnsupportedEndianness(_)
            | Self::UnsupportedHeaderVersion(_)
            | Self::UnsupportedFileType(_)
            | Self::UnsupportedMachine(_)
            | Self::UnsupportedFeature(_) => {
                Diagnostic::new("PFG1001", DiagnosticClass::Unsupported)
            }
            Self::Truncated(_)
            | Self::InvalidStructureSize { .. }
            | Self::InvalidProgramHeaderTable
            | Self::InvalidLoadSegment(_)
            | Self::NoLoadSegments => Diagnostic::new("PFG2001", DiagnosticClass::Corrupt),
        }
    }
}

impl ContainerError {
    /// Returns the stable diagnostic mapping for a container failure.
    #[must_use]
    pub const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Format(error) => error.diagnostic(),
            Self::SizeLimit { .. }
            | Self::Allocation { .. }
            | Self::DecoderWindowLimit { .. }
            | Self::InvalidIterations { .. } => {
                Diagnostic::new("PFG3001", DiagnosticClass::ResourceLimit)
            }
            Self::UnsupportedVersion(_)
            | Self::InvalidHeaderLength(_)
            | Self::NonzeroReserved
            | Self::UnknownCodec(_)
            | Self::UnknownProfile(_)
            | Self::UnknownFormat(_)
            | Self::UnknownClass(_)
            | Self::UnknownEndianness(_)
            | Self::UnsupportedEmbeddedMetadata(_) => {
                Diagnostic::new("PFG1002", DiagnosticClass::Unsupported)
            }
            Self::NotBeneficial { .. } => Diagnostic::new("PFG1003", DiagnosticClass::Conflict),
            Self::HeaderIntegrity
            | Self::PayloadIntegrity
            | Self::OriginalIntegrity
            | Self::MetadataMismatch(_) => Diagnostic::new("PFG2002", DiagnosticClass::Corrupt),
            Self::Decompression(_) | Self::OriginalLength { .. } => {
                Diagnostic::new("PFG2003", DiagnosticClass::Corrupt)
            }
            Self::TruncatedHeader | Self::InvalidMagic | Self::InvalidContainerLength { .. } => {
                Diagnostic::new("PFG2001", DiagnosticClass::Corrupt)
            }
            Self::Compression(_) | Self::NonDeterministic(_) => INTERNAL_DIAGNOSTIC,
        }
    }
}

impl ExecutableError {
    /// Returns the stable diagnostic mapping for a native-wrapper failure.
    #[must_use]
    pub const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Container(error) => error.diagnostic(),
            Self::LoaderFormat(_)
            | Self::UnsupportedVersion(_)
            | Self::InvalidTrailerLength(_)
            | Self::UnsupportedRuntimeAbi(_)
            | Self::NonzeroReserved
            | Self::UnsupportedTarget { .. }
            | Self::UnsupportedRuntimeProfile(_) => {
                Diagnostic::new("PFG1002", DiagnosticClass::Unsupported)
            }
            Self::NotBeneficial { .. } => Diagnostic::new("PFG1003", DiagnosticClass::Conflict),
            Self::InvalidLoaderLength { .. } | Self::SizeOverflow(_) => {
                Diagnostic::new("PFG3001", DiagnosticClass::ResourceLimit)
            }
            Self::TrailerIntegrity | Self::LoaderIntegrity => {
                Diagnostic::new("PFG2002", DiagnosticClass::Corrupt)
            }
            Self::TruncatedTrailer
            | Self::InvalidMagic
            | Self::InvalidExecutableLength { .. }
            | Self::InvalidContainerRange => Diagnostic::new("PFG2001", DiagnosticClass::Corrupt),
        }
    }
}

impl Error {
    /// Returns the stable diagnostic mapping for a host-side operation failure.
    #[must_use]
    pub const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Container(error) => error.diagnostic(),
            Self::Executable(error) => error.diagnostic(),
            Self::Io { .. } | Self::NotRegularFile(_) => {
                Diagnostic::new("PFG4001", DiagnosticClass::Io)
            }
            Self::OutputExists(_) => Diagnostic::new("PFG4002", DiagnosticClass::Conflict),
            Self::UnknownArtifact | Self::InputNotExecutable(_) => {
                Diagnostic::new("PFG1001", DiagnosticClass::Unsupported)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ContainerError, DiagnosticClass, Error, FormatError};

    #[test]
    fn unsupported_corrupt_and_resource_failures_are_distinct() {
        let unsupported = Error::from(ContainerError::Format(FormatError::NotElf)).diagnostic();
        assert_eq!(unsupported.code, "PFG1001");
        assert_eq!(unsupported.class, DiagnosticClass::Unsupported);
        assert_eq!(unsupported.exit_code(), 3);

        let corrupt = Error::from(ContainerError::PayloadIntegrity).diagnostic();
        assert_eq!(corrupt.code, "PFG2002");
        assert_eq!(corrupt.class, DiagnosticClass::Corrupt);
        assert_eq!(corrupt.exit_code(), 4);

        let limited = Error::from(ContainerError::SizeLimit {
            field: "input",
            actual: 2,
            maximum: 1,
        })
        .diagnostic();
        assert_eq!(limited.code, "PFG3001");
        assert_eq!(limited.class, DiagnosticClass::ResourceLimit);
        assert_eq!(limited.exit_code(), 5);
    }
}
