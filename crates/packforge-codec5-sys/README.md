# Packforge codec 5 host bridge

This private crate compiles the APultra encoder pinned at commit
`8f340057d7402c10da3d9c76c599f9ab83b8a22d` and the public-domain BCJ2 codec
from 7-Zip 26.02 commit `f9d78aff31a5f2521ae7ddbdc97c4a8855808959`.
APultra is zlib-licensed and its suffix-array match finder includes CC0 code.
The exact upstream notices are retained under `vendor/apultra`.

Only safe, bounded Rust functions are exposed to `packforge-core`. Runtime code
does not link these host objects; it uses a separately checked no-allocator
decoder.
