#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use packforge_core::{
    ArtifactInfo, ArtifactReport, Diagnostic, Error as CoreError, ExecutableInfo, ExecutableV2Info,
    INTERNAL_DIAGNOSTIC, PackOptions, Profile,
};

/// A modern, transparent executable packer.
#[derive(Debug, Parser)]
#[command(version, about, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the current implementation milestone.
    Status,
    /// Create a deterministic recovery container or self-contained executable.
    Pack {
        /// Static ELF x86-64 executable to pack.
        input: PathBuf,
        /// Output path; defaults to INPUT.pfg or INPUT.packed by artifact kind.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Artifact kind; native executable output is opt-in during the runtime spike.
        #[arg(long, value_enum, default_value_t = CliArtifact::Container)]
        artifact: CliArtifact,
        /// Size/startup policy; defaults to balanced.
        #[arg(long, value_enum)]
        profile: Option<CliProfile>,
        /// Keep the selected artifact even when it is not smaller than the input.
        #[arg(long)]
        allow_larger: bool,
        /// Emit the operation report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Recover a byte-identical executable from either Packforge artifact kind.
    Unpack {
        /// Packforge container or self-contained executable to unpack.
        input: PathBuf,
        /// Output path; removes .pfg/.packed when present, otherwise adds .unpacked.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Emit the operation report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Validate and display wrapper, header, and compressed-payload metadata.
    Inspect {
        /// Packforge container or self-contained executable to inspect.
        input: PathBuf,
        /// Emit the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Fully validate either Packforge artifact kind without writing output.
    Verify {
        /// Packforge container or self-contained executable to verify.
        input: PathBuf,
        /// Emit the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Measure every stable profile and verify deterministic output.
    Benchmark {
        /// Static ELF x86-64 executable to benchmark.
        input: PathBuf,
        /// Measured iterations per profile, after one warm-up.
        #[arg(long, default_value_t = 5)]
        iterations: u32,
        /// Emit the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliProfile {
    Fast,
    Balanced,
    Small,
    Auto,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliArtifact {
    Container,
    Executable,
}

impl From<CliProfile> for Profile {
    fn from(profile: CliProfile) -> Self {
        match profile {
            CliProfile::Fast => Self::Fast,
            CliProfile::Balanced => Self::Balanced,
            CliProfile::Small => Self::Small,
            CliProfile::Auto => Self::Auto,
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let diagnostic = error.diagnostic();
            eprintln!("error[{}]: {error}", diagnostic.code);
            ExitCode::from(diagnostic.exit_code())
        }
    }
}

#[derive(Debug)]
enum CliError {
    Core(CoreError),
    Json(serde_json::Error),
    Output(io::Error),
}

impl CliError {
    const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Core(error) => error.diagnostic(),
            Self::Json(_) | Self::Output(_) => INTERNAL_DIAGNOSTIC,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "could not serialize JSON report: {error}"),
            Self::Output(error) => write!(formatter, "could not write command output: {error}"),
        }
    }
}

impl From<CoreError> for CliError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Status => {
            println!("{}", packforge_core::project_stage().as_str());
            Ok(())
        }
        Command::Pack {
            input,
            output,
            artifact,
            profile,
            allow_larger,
            json,
        } => {
            let output = output.unwrap_or_else(|| default_pack_output(&input, artifact));
            let options = PackOptions {
                profile: profile.map_or(Profile::Balanced, Profile::from),
                allow_larger,
            };
            let report = match artifact {
                CliArtifact::Container => ArtifactReport::Container {
                    info: packforge_core::pack_file(&input, &output, options)?,
                },
                CliArtifact::Executable => ArtifactReport::ExecutableV2 {
                    info: packforge_core::pack_executable_v2_file(&input, &output, options)?,
                },
            };
            if json {
                print_json(&report)?;
            } else {
                println!("packed {} -> {}", input.display(), output.display());
                print_report(&report);
            }
            Ok(())
        }
        Command::Unpack {
            input,
            output,
            json,
        } => {
            let output = output.unwrap_or_else(|| default_unpack_output(&input));
            let report = packforge_core::unpack_artifact_file(&input, &output)?;
            if json {
                print_json(&report)?;
            } else {
                println!("unpacked {} -> {}", input.display(), output.display());
                print_report(&report);
            }
            Ok(())
        }
        Command::Inspect { input, json } => {
            let report = packforge_core::inspect_artifact_file(&input)?;
            if json {
                print_json(&report)?;
            } else {
                print_report(&report);
            }
            Ok(())
        }
        Command::Verify { input, json } => {
            let report = packforge_core::verify_artifact_file(&input)?;
            if json {
                print_json(&report)?;
            } else {
                println!("verified {}", input.display());
                print_report(&report);
            }
            Ok(())
        }
        Command::Benchmark {
            input,
            iterations,
            json,
        } => run_benchmark(&input, iterations, json),
    }
}

fn run_benchmark(input: &Path, iterations: u32, json: bool) -> Result<(), CliError> {
    let report = packforge_core::benchmark_file(input, iterations)?;
    if json {
        return print_json(&report);
    }

    println!("benchmark {}", input.display());
    println!("  iterations {} + 1 warm-up", report.iterations);
    println!("  original   {} bytes", report.original_size);
    println!();
    println!(
        "{:<10} {:<8} {:>12} {:>10} {:>12} {:>12}",
        "profile", "codec", "container", "ratio", "pack median", "verify median"
    );
    for sample in report.profiles {
        let ratio_whole = sample.payload_ratio_basis_points / 100;
        let ratio_fraction = sample.payload_ratio_basis_points % 100;
        let ratio = format!("{ratio_whole}.{ratio_fraction:02}%");
        println!(
            "{:<10} {:<8} {:>12} {:>10} {:>9.3} ms {:>9.3} ms",
            sample.profile.as_str(),
            sample.codec.as_str(),
            sample.container_size,
            ratio,
            nanoseconds_to_milliseconds(sample.pack_nanoseconds_median),
            nanoseconds_to_milliseconds(sample.verify_nanoseconds_median),
        );
    }
    Ok(())
}

fn print_json(value: &impl serde::Serialize) -> Result<(), CliError> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(CliError::Json)?;
    output.write_all(b"\n").map_err(CliError::Output)?;
    Ok(())
}

fn nanoseconds_to_milliseconds(nanoseconds: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let nanoseconds = nanoseconds as f64;
    nanoseconds / 1_000_000.0
}

fn print_human(info: &ArtifactInfo) {
    let ratio_whole = info.payload_ratio_basis_points / 100;
    let ratio_fraction = info.payload_ratio_basis_points % 100;
    println!("  profile       {}", info.profile.as_str());
    println!(
        "  codec         {} ({})",
        info.codec.as_str(),
        info.codec_level
    );
    println!("  original      {} bytes", info.original_size);
    println!("  payload       {} bytes", info.payload_size);
    println!("  container     {} bytes", info.container_size);
    println!("  payload ratio {ratio_whole}.{ratio_fraction:02}%");
    println!("  architecture  x86_64-linux-elf");
    println!("  entry point   0x{:x}", info.binary.entry_point);
    println!("  load segments {}", info.binary.load_segments);
    println!("  original hash {}", info.original_digest);
    println!("  payload hash  {}", info.payload_digest);
}

fn print_report(report: &ArtifactReport) {
    match report {
        ArtifactReport::Container { info } => {
            println!("  artifact      container");
            print_human(info);
        }
        ArtifactReport::Executable { info } => print_executable(info),
        ArtifactReport::ExecutableV2 { info } => print_executable_v2(info),
    }
}

fn print_executable(info: &ExecutableInfo) {
    println!("  artifact      executable");
    println!("  wrapper       v{}", info.executable_version);
    println!("  runtime ABI   v{}", info.runtime_abi_version);
    println!("  loader        {} bytes", info.loader_size);
    println!("  executable    {} bytes", info.executable_size);
    println!("  loader hash   {}", info.loader_digest);
    print_human(&info.container);
}

fn print_executable_v2(info: &ExecutableV2Info) {
    println!("  artifact      executable");
    println!("  wrapper       v{}", info.executable_version);
    println!("  runtime ABI   v{}", info.runtime_abi_version);
    println!("  codec         {}", info.codec);
    println!("  loader        {} bytes", info.loader_size);
    println!("  manifest      {} bytes", info.manifest_size);
    println!("  payload       {} bytes", info.payload_size);
    println!("  original      {} bytes", info.original_size);
    println!("  executable    {} bytes", info.executable_size);
    println!("  loader hash   {}", info.loader_digest);
    println!("  manifest hash {}", info.manifest_digest);
    println!("  payload hash  {}", info.payload_digest);
    println!("  original hash {}", info.original_digest);
}

fn default_pack_output(input: &Path, artifact: CliArtifact) -> PathBuf {
    let mut output = OsString::from(input.as_os_str());
    output.push(match artifact {
        CliArtifact::Container => ".pfg",
        CliArtifact::Executable => ".packed",
    });
    PathBuf::from(output)
}

fn default_unpack_output(input: &Path) -> PathBuf {
    let stripped = input
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| {
            name.strip_suffix(".pfg")
                .or_else(|| name.strip_suffix(".packed"))
        })
        .filter(|stripped| !stripped.is_empty());
    if let Some(stripped) = stripped {
        return input.with_file_name(stripped);
    }
    let mut output = OsString::from(input.as_os_str());
    output.push(".unpacked");
    PathBuf::from(output)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CliArtifact, default_pack_output, default_unpack_output};

    #[test]
    fn derives_pack_output_without_losing_extension() {
        assert_eq!(
            default_pack_output(Path::new("hello.bin"), CliArtifact::Container),
            Path::new("hello.bin.pfg")
        );
        assert_eq!(
            default_pack_output(Path::new("hello.bin"), CliArtifact::Executable),
            Path::new("hello.bin.packed")
        );
    }

    #[test]
    fn derives_unpack_output_from_container_suffix() {
        assert_eq!(
            default_unpack_output(Path::new("hello.bin.pfg")),
            Path::new("hello.bin")
        );
        assert_eq!(
            default_unpack_output(Path::new("hello.bin.packed")),
            Path::new("hello.bin")
        );
    }
}
