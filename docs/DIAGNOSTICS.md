# CLI diagnostics contract

Packforge writes failures as `error[CODE]: message` on standard error. The code
and process status are the automation contract; message wording may gain detail
without a schema-version change. Successful commands return `0`, and Clap-owned
command-line usage errors return `2`.

| Code | Class | Exit | Meaning |
| --- | --- | ---: | --- |
| `PFG1001` | unsupported | 3 | Input format, permissions, or executable feature is outside the supported tier. |
| `PFG1002` | unsupported | 3 | Artifact version, runtime ABI, codec, target, or embedded feature is unsupported. |
| `PFG1003` | conflict | 7 | Packing would not reduce size without `--allow-larger`. |
| `PFG2001` | corrupt | 4 | Artifact framing, range, length, or source executable structure is malformed. |
| `PFG2002` | corrupt | 4 | A header, payload, loader, trailer, or reconstructed-image integrity check failed. |
| `PFG2003` | corrupt | 4 | Bounded decompression failed or produced a different declared length. |
| `PFG3001` | resource_limit | 5 | An input, declared length, decoder window, or iteration count exceeds a hard bound. |
| `PFG4001` | io | 6 | A filesystem metadata, read, write, permission, or synchronization operation failed. |
| `PFG4002` | conflict | 7 | The output already exists and Packforge refused to clobber it. |
| `PFG5001` | internal | 70 | Serialization, command output, compression, or determinism failed internally. |

Unsupported and corrupt inputs are deliberately distinct. An unsupported result
means a structurally understood request is outside the current compatibility
tier. A corrupt result means the claimed structure cannot be trusted. Resource
limits are checked before attacker-controlled allocation or decompression.

Existing codes and classes are never repurposed. New failure distinctions receive
new codes; backward-compatible message clarification does not require a new code.

## JSON report contract

Container `pack --json`, `inspect --json`, `verify --json`, and `unpack --json`
reports use [`container-report-v1.schema.json`](../schemas/container-report-v1.schema.json).
The checked-in [inspect](../schemas/examples/container-inspect-v1.json) and
[verify](../schemas/examples/container-verify-v1.json) reports are generated from
the same deterministic synthetic ELF fixture used by the contract tests.
