//! Native-corpus payload probe for the clean-room `PFG-LZ/1` feasibility gate.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use packforge_core::{
    LINUX_X86_64_RUNTIME_V2_CODEC5, PackOptions, pack_executable_v2_codec5, pfg_lz,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("m2_pfg_lz_probe: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let fixture = arguments
        .next()
        .ok_or_else(|| usage(&program))?
        .to_string_lossy()
        .into_owned();
    let input_path = arguments.next().ok_or_else(|| usage(&program))?;
    if arguments.next().is_some() {
        return Err(usage(&program));
    }

    let original = fs::read(&input_path).map_err(|error| {
        format!(
            "could not read {}: {error}",
            Path::new(&input_path).display()
        )
    })?;
    let payload = pfg_lz::encode(&original);
    if pfg_lz::decode(&payload, original.len()).map_err(|error| error.to_string())? != original {
        return Err("PFG-LZ round trip did not recover the original".to_owned());
    }
    let codec5 = pack_executable_v2_codec5(
        &original,
        0o755,
        PackOptions {
            profile: packforge_core::Profile::Balanced,
            allow_larger: true,
        },
        LINUX_X86_64_RUNTIME_V2_CODEC5,
    )
    .map_err(|error| format!("codec-5 comparison pack failed: {error}"))?;
    let ratio_basis_points = ratio_basis_points(payload.len(), codec5.info.payload_size)
        .ok_or_else(|| "codec-5 payload has zero length".to_owned())?;
    println!(
        "{fixture}\t{}\t{}\t{}\t{ratio_basis_points}",
        original.len(),
        payload.len(),
        codec5.info.payload_size
    );
    Ok(())
}

fn ratio_basis_points(numerator: usize, denominator: u64) -> Option<u64> {
    let denominator = u128::from(denominator);
    if denominator == 0 {
        return None;
    }
    u64::try_from((numerator as u128) * 10_000 / denominator).ok()
}

fn usage(program: &std::ffi::OsStr) -> String {
    format!(
        "usage: {} <fixture> <original>",
        Path::new(program).display()
    )
}
