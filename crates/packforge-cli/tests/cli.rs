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
    assert!(String::from_utf8_lossy(&output.stderr).contains("not an ELF executable"));
}
