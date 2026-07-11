use std::env;
use std::process::ExitCode;

fn checksum(text: &str) -> u32 {
    let mut value = 2_166_136_261u32;
    for byte in text.bytes() {
        value ^= u32::from(byte);
        value = value.wrapping_mul(16_777_619);
    }
    value
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().collect();
    let environment = env::var("PACKFORGE_SMOKE").ok();
    if arguments.len() != 2 || environment.is_none() {
        eprintln!("expected one argument and PACKFORGE_SMOKE");
        return ExitCode::from(2);
    }

    let argument = &arguments[1];
    let environment = environment.unwrap();
    println!(
        "packforge-smoke argc={} arg={} env={} checksum={}",
        arguments.len(),
        argument,
        environment,
        checksum(argument) ^ checksum(&environment)
    );
    if argument == "round-trip" {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}
