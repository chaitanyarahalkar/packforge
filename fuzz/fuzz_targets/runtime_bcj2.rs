#![no_main]

use libfuzzer_sys::fuzz_target;
use packforge_runtime_linux_x86_64::bcj2;

const MAX_FUZZ_OUTPUT: usize = 1 << 20;

fuzz_target!(|input: &[u8]| {
    let Some(header) = input.get(..16) else {
        return;
    };
    let body = &input[16..];
    let mut cursor = 0usize;
    let main = take(body, &mut cursor, stream_length(header, 0));
    let call = take(body, &mut cursor, stream_length(header, 4));
    let jump = take(body, &mut cursor, stream_length(header, 8));
    let range = &body[cursor..];
    let requested =
        u32::from_le_bytes(header[12..16].try_into().expect("fixed output length")) as usize;
    let mut output = vec![0u8; requested % (MAX_FUZZ_OUTPUT + 1)];
    let _ = bcj2::decode(main, call, jump, range, &mut output);
});

fn stream_length(header: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(
        header[offset..offset + 4]
            .try_into()
            .expect("fixed stream length"),
    ) as usize
}

fn take<'a>(body: &'a [u8], cursor: &mut usize, requested: usize) -> &'a [u8] {
    let length = requested % (body.len() - *cursor + 1);
    let bytes = &body[*cursor..*cursor + length];
    *cursor += length;
    bytes
}
