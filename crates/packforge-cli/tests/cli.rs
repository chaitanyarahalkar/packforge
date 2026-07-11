use std::fs;
use std::process::{Command, Output};

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0u8; 16_384];
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
    bytes[96..104].copy_from_slice(&16_384u64.to_le_bytes());
    bytes[104..112].copy_from_slice(&16_384u64.to_le_bytes());
    bytes[256..].fill(0x41);
    bytes
}

fn packforge(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_packforge"))
        .args(arguments)
        .output()
        .expect("Packforge CLI should launch")
}

#[test]
fn cli_round_trip_and_json_reports() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture");
    let container = directory.path().join("fixture.pfg");
    let restored = directory.path().join("restored");
    fs::write(&input, fixture()).unwrap();

    let packed = packforge(&[
        "pack",
        input.to_str().unwrap(),
        "--output",
        container.to_str().unwrap(),
        "--profile",
        "auto",
        "--json",
    ]);
    assert!(
        packed.status.success(),
        "{}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let packed_report: serde_json::Value = serde_json::from_slice(&packed.stdout).unwrap();
    assert_eq!(packed_report["artifact_kind"], "container");
    assert_eq!(packed_report["profile"], "auto");
    assert_eq!(packed_report["verification"], "full");

    let inspected = packforge(&["inspect", container.to_str().unwrap(), "--json"]);
    assert!(inspected.status.success());
    let inspected_report: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected_report["verification"], "payload");

    let verified = packforge(&["verify", container.to_str().unwrap(), "--json"]);
    assert!(verified.status.success());
    let verified_report: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(verified_report["verification"], "full");

    let unpacked = packforge(&[
        "unpack",
        container.to_str().unwrap(),
        "--output",
        restored.to_str().unwrap(),
        "--json",
    ]);
    assert!(unpacked.status.success());
    assert_eq!(fs::read(&restored).unwrap(), fixture());
}

#[cfg(unix)]
#[test]
fn cli_self_contained_executable_round_trips_without_execution() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture");
    let executable = directory.path().join("fixture.packed");
    let restored = directory.path().join("restored");
    fs::write(&input, fixture()).unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o755)).unwrap();

    let packed = packforge(&[
        "pack",
        input.to_str().unwrap(),
        "--output",
        executable.to_str().unwrap(),
        "--artifact",
        "executable",
        "--allow-larger",
        "--json",
    ]);
    assert!(
        packed.status.success(),
        "{}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&packed.stdout).unwrap();
    assert_eq!(report["artifact_kind"], "executable");
    assert_eq!(report["executable_version"], 1);
    assert_eq!(report["runtime_abi_version"], 1);
    assert_eq!(report["container"]["profile"], "fast");

    let inspected = packforge(&["inspect", executable.to_str().unwrap(), "--json"]);
    assert!(inspected.status.success());
    let report: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(report["artifact_kind"], "executable");
    assert_eq!(report["verification"], "payload");

    let verified = packforge(&["verify", executable.to_str().unwrap(), "--json"]);
    assert!(verified.status.success());
    let report: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(report["verification"], "full");

    let unpacked = packforge(&[
        "unpack",
        executable.to_str().unwrap(),
        "--output",
        restored.to_str().unwrap(),
        "--json",
    ]);
    assert!(unpacked.status.success());
    assert_eq!(fs::read(restored).unwrap(), fixture());
}

#[test]
fn cli_refuses_to_clobber_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture");
    let container = directory.path().join("fixture.pfg");
    fs::write(&input, fixture()).unwrap();
    fs::write(&container, b"existing").unwrap();

    let output = packforge(&[
        "pack",
        input.to_str().unwrap(),
        "--output",
        container.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(7));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error[PFG4002]:"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to overwrite"));
    assert_eq!(fs::read(&container).unwrap(), b"existing");
}

#[test]
fn cli_rejects_non_elf_input() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("not-elf");
    fs::write(&input, b"plain text is not an executable").unwrap();

    let output = packforge(&["pack", input.to_str().unwrap(), "--allow-larger"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error[PFG1001]:"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not an ELF executable"));
}

#[test]
fn cli_distinguishes_corrupt_artifacts_from_unsupported_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture");
    let container = directory.path().join("fixture.pfg");
    fs::write(&input, fixture()).unwrap();
    let packed = packforge(&[
        "pack",
        input.to_str().unwrap(),
        "--output",
        container.to_str().unwrap(),
        "--allow-larger",
    ]);
    assert!(packed.status.success());

    let mut bytes = fs::read(&container).unwrap();
    let last = bytes.last_mut().unwrap();
    *last ^= 0x80;
    fs::write(&container, bytes).unwrap();

    let output = packforge(&["inspect", container.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error[PFG2002]:"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("payload checksum mismatch"));
}

#[test]
fn cli_reports_bounded_requests_as_resource_failures() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture");
    fs::write(&input, fixture()).unwrap();

    let output = packforge(&["benchmark", input.to_str().unwrap(), "--iterations", "0"]);
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error[PFG3001]:"));
}

#[test]
fn cli_benchmark_reports_all_profiles() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture");
    fs::write(&input, fixture()).unwrap();

    let output = packforge(&[
        "benchmark",
        input.to_str().unwrap(),
        "--iterations",
        "1",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["iterations"], 1);
    assert_eq!(report["warmup_iterations"], 1);
    assert_eq!(report["profiles"].as_array().unwrap().len(), 4);
}
