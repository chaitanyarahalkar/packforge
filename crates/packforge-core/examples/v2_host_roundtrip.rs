use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use packforge_core::{
    LINUX_X86_64_RUNTIME_V2, PackOptions, Profile, inspect_executable_v2, pack_executable_v2,
    unpack_executable_v2, verify_executable_v2,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inputs: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if inputs.is_empty() {
        return Err("usage: v2_host_roundtrip INPUT...".into());
    }
    for input_path in inputs {
        let original = fs::read(&input_path)?;
        let mode = fs::metadata(&input_path)?.permissions().mode();
        let options = PackOptions {
            profile: Profile::Balanced,
            allow_larger: true,
        };
        let first = pack_executable_v2(&original, mode, options, LINUX_X86_64_RUNTIME_V2)?;
        let second = pack_executable_v2(&original, mode, options, LINUX_X86_64_RUNTIME_V2)?;
        if first.bytes != second.bytes {
            return Err(format!(
                "{} produced nondeterministic v2 bytes",
                input_path.display()
            )
            .into());
        }
        inspect_executable_v2(&first.bytes)?;
        verify_executable_v2(&first.bytes)?;
        let unpacked = unpack_executable_v2(&first.bytes)?;
        if unpacked.bytes != original || unpacked.info.original_mode != mode {
            return Err(format!("{} failed v2 recovery", input_path.display()).into());
        }
        println!(
            "{}\t{}\t{}\t{}",
            input_path.display(),
            original.len(),
            first.info.payload_size,
            first.bytes.len()
        );
    }
    Ok(())
}
