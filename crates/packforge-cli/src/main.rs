use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use packforge_core::{ArtifactInfo, PackOptions, Profile};

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
    /// Create a deterministic reversible container.
    Pack {
        /// Static ELF x86-64 executable to pack.
        input: PathBuf,
        /// Output path; defaults to INPUT.pfg.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Size/startup compression policy.
        #[arg(long, value_enum, default_value_t = CliProfile::Balanced)]
        profile: CliProfile,
        /// Keep the container even when it is not smaller than the input.
        #[arg(long)]
        allow_larger: bool,
        /// Emit the operation report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Recover a byte-identical executable from a container.
    Unpack {
        /// Packforge container to unpack.
        input: PathBuf,
        /// Output path; defaults to INPUT without .pfg.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Emit the operation report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Validate and display header and compressed-payload metadata.
    Inspect {
        /// Packforge container to inspect.
        input: PathBuf,
        /// Emit the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Decompress and fully validate a container without writing output.
    Verify {
        /// Packforge container to verify.
        input: PathBuf,
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
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Status => {
            println!("{}", packforge_core::project_stage().as_str());
            Ok(())
        }
        Command::Pack {
            input,
            output,
            profile,
            allow_larger,
            json,
        } => {
            let output = output.unwrap_or_else(|| default_pack_output(&input));
            let info = packforge_core::pack_file(
                &input,
                &output,
                PackOptions {
                    profile: profile.into(),
                    allow_larger,
                },
            )
            .map_err(|error| error.to_string())?;
            if json {
                print_json(&info)?;
            } else {
                println!("packed {} -> {}", input.display(), output.display());
                print_human(&info);
            }
            Ok(())
        }
        Command::Unpack {
            input,
            output,
            json,
        } => {
            let output = output.unwrap_or_else(|| default_unpack_output(&input));
            let info =
                packforge_core::unpack_file(&input, &output).map_err(|error| error.to_string())?;
            if json {
                print_json(&info)?;
            } else {
                println!("unpacked {} -> {}", input.display(), output.display());
                print_human(&info);
            }
            Ok(())
        }
        Command::Inspect { input, json } => {
            let info = packforge_core::inspect_file(&input).map_err(|error| error.to_string())?;
            if json {
                print_json(&info)?;
            } else {
                print_human(&info);
            }
            Ok(())
        }
        Command::Verify { input, json } => {
            let info = packforge_core::verify_file(&input).map_err(|error| error.to_string())?;
            if json {
                print_json(&info)?;
            } else {
                println!("verified {}", input.display());
                print_human(&info);
            }
            Ok(())
        }
    }
}

fn print_json(info: &ArtifactInfo) -> Result<(), String> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, info).map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    Ok(())
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

fn default_pack_output(input: &Path) -> PathBuf {
    let mut output = OsString::from(input.as_os_str());
    output.push(".pfg");
    PathBuf::from(output)
}

fn default_unpack_output(input: &Path) -> PathBuf {
    if let Some(name) = input.file_name().and_then(|name| name.to_str())
        && let Some(stripped) = name.strip_suffix(".pfg")
        && !stripped.is_empty()
    {
        return input.with_file_name(stripped);
    }
    let mut output = OsString::from(input.as_os_str());
    output.push(".unpacked");
    PathBuf::from(output)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{default_pack_output, default_unpack_output};

    #[test]
    fn derives_pack_output_without_losing_extension() {
        assert_eq!(
            default_pack_output(Path::new("hello.bin")),
            Path::new("hello.bin.pfg")
        );
    }

    #[test]
    fn derives_unpack_output_from_container_suffix() {
        assert_eq!(
            default_unpack_output(Path::new("hello.bin.pfg")),
            Path::new("hello.bin")
        );
    }
}
