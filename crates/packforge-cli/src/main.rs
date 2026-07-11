use std::process::ExitCode;

const HELP: &str = "Packforge planning scaffold

Usage:
  packforge status
  packforge --help
  packforge --version

Packing commands will be introduced after the manifest and round-trip container
milestones are complete.";

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _program = args.next();
    let command = args.next();

    if args.next().is_some() {
        eprintln!("error: expected exactly one command\n\n{HELP}");
        return ExitCode::from(2);
    }

    match command.as_deref().and_then(|value| value.to_str()) {
        Some("status") => {
            println!("{}", packforge_core::project_stage().as_str());
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("packforge {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(command) => {
            eprintln!("error: unknown command `{command}`\n\n{HELP}");
            ExitCode::from(2)
        }
    }
}
