use std::collections::BTreeSet;

use packforge_core::{ArtifactReport, PackOptions, Profile, inspect, pack, verify};
use serde_json::{Map, Value};

const SCHEMA: &str = include_str!("../../../schemas/container-report-v1.schema.json");
const INSPECT_GOLDEN: &str = include_str!("../../../schemas/examples/container-inspect-v1.json");
const VERIFY_GOLDEN: &str = include_str!("../../../schemas/examples/container-verify-v1.json");

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

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("container report schema must be JSON")
}

fn report(verification: &str) -> Value {
    let packed = pack(
        &fixture(),
        0o755,
        PackOptions {
            profile: Profile::Auto,
            allow_larger: false,
        },
    )
    .unwrap();
    let info = match verification {
        "payload" => inspect(&packed.bytes).unwrap(),
        "full" => verify(&packed.bytes).unwrap(),
        _ => panic!("unknown verification depth"),
    };
    serde_json::to_value(ArtifactReport::Container { info }).unwrap()
}

fn object(value: &Value) -> &Map<String, Value> {
    value.as_object().expect("contract value must be an object")
}

fn validate(
    schema_root: &Value,
    contract: &Value,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    if let Some(reference) = contract.get("$ref").and_then(Value::as_str) {
        let pointer = reference
            .strip_prefix('#')
            .ok_or_else(|| format!("{path}: only local schema references are supported"))?;
        let resolved = schema_root
            .pointer(pointer)
            .ok_or_else(|| format!("{path}: unresolved schema reference {reference}"))?;
        return validate(schema_root, resolved, value, path);
    }

    if let Some(expected) = contract.get("const") {
        if value != expected {
            return Err(format!("{path}: expected constant {expected}, got {value}"));
        }
    }
    if let Some(allowed) = contract.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            return Err(format!(
                "{path}: {value} is outside the allowed enumeration"
            ));
        }
    }
    if let Some(kind) = contract.get("type").and_then(Value::as_str) {
        let matches = match kind {
            "object" => value.is_object(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            _ => return Err(format!("{path}: unsupported test-schema type {kind}")),
        };
        if !matches {
            return Err(format!("{path}: expected {kind}, got {value}"));
        }
    }

    validate_scalar(contract, value, path)?;
    if contract.get("type").and_then(Value::as_str) == Some("object") {
        validate_object(schema_root, contract, value, path)?;
    }
    Ok(())
}

fn validate_scalar(contract: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(minimum) = contract.get("minimum").and_then(Value::as_i64) {
        let actual = value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .ok_or_else(|| format!("{path}: minimum applies to a non-integer"))?;
        if actual < minimum {
            return Err(format!("{path}: {actual} is below {minimum}"));
        }
    }
    if let Some(maximum) = contract.get("maximum").and_then(Value::as_u64) {
        let actual = value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
            .ok_or_else(|| format!("{path}: maximum applies to a negative/non-integer"))?;
        if actual > maximum {
            return Err(format!("{path}: {actual} exceeds {maximum}"));
        }
    }
    if contract.get("pattern").is_some() {
        let text = value
            .as_str()
            .ok_or_else(|| format!("{path}: digest pattern applies to a non-string"))?;
        if text.len() != 64
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(format!(
                "{path}: expected 64 lowercase hexadecimal characters"
            ));
        }
    }
    Ok(())
}

fn validate_object(
    schema_root: &Value,
    contract: &Value,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    let actual = object(value);
    let required: BTreeSet<_> = contract
        .get("required")
        .and_then(Value::as_array)
        .expect("object contract must have required properties")
        .iter()
        .map(|name| name.as_str().expect("required names must be strings"))
        .collect();
    let properties = object(
        contract
            .get("properties")
            .expect("object contract must define properties"),
    );
    let declared: BTreeSet<_> = properties.keys().map(String::as_str).collect();
    if required != declared {
        return Err(format!(
            "{path}: required and declared property sets differ"
        ));
    }
    let actual_keys: BTreeSet<_> = actual.keys().map(String::as_str).collect();
    if actual_keys != required {
        return Err(format!(
            "{path}: expected fields {required:?}, got {actual_keys:?}"
        ));
    }
    if contract.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(format!(
            "{path}: contract must reject additional properties"
        ));
    }
    for (name, property_contract) in properties {
        validate(
            schema_root,
            property_contract,
            &actual[name],
            &format!("{path}.{name}"),
        )?;
    }
    Ok(())
}

#[test]
fn inspect_and_verify_reports_match_schema_and_goldens() {
    let schema = schema();
    for (depth, golden) in [("payload", INSPECT_GOLDEN), ("full", VERIFY_GOLDEN)] {
        let actual = report(depth);
        let expected: Value = serde_json::from_str(golden).unwrap();
        assert_eq!(
            actual, expected,
            "{depth} report changed without a schema bump"
        );
        validate(&schema, &schema, &actual, "$").unwrap();
    }
}

#[test]
fn compatibility_validator_rejects_removed_and_added_fields() {
    let schema = schema();
    let mut removed = report("full");
    object(&removed);
    removed.as_object_mut().unwrap().remove("payload_digest");
    assert!(validate(&schema, &schema, &removed, "$").is_err());

    let mut added = report("full");
    added
        .as_object_mut()
        .unwrap()
        .insert("future_field".into(), Value::Bool(true));
    assert!(validate(&schema, &schema, &added, "$").is_err());
}
