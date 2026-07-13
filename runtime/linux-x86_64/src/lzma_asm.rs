//! Minimal fixed-profile driver for 7-Zip's public-domain x86-64 decoder core.

use core::arch::{asm, global_asm};
use core::mem::{offset_of, size_of};

global_asm!(
    r#"
    .hidden LzmaDec_DecodeReal_3
    .hidden packforge_lzma_finish_remaining
    .global packforge_lzma_finish_remaining
    .type packforge_lzma_finish_remaining,@function
packforge_lzma_finish_remaining:
    mov 40(%rdi), %rax
    cmp %rsi, %rax
    jae .Lrem_ok
    mov 92(%rdi), %ecx
    test %ecx, %ecx
    jz .Lrem_ok
    cmp $273, %ecx
    ja .Lrem_error
    mov 72(%rdi), %edx
    test %edx, %edx
    jz .Lrem_error
    cmp %rdx, %rax
    jb .Lrem_error
    mov 24(%rdi), %r8
.Lrem_loop:
    mov %rax, %r9
    sub %rdx, %r9
    mov (%r8,%r9), %r10b
    mov %r10b, (%r8,%rax)
    inc %rax
    dec %ecx
    cmp %rsi, %rax
    jae .Lrem_done
    test %ecx, %ecx
    jnz .Lrem_loop
.Lrem_done:
    mov %rax, 40(%rdi)
    mov %ecx, 92(%rdi)
.Lrem_ok:
    xor %eax, %eax
    ret
.Lrem_error:
    mov $1, %eax
    ret
    .size packforge_lzma_finish_remaining, .-packforge_lzma_finish_remaining
"#,
    options(att_syntax)
);

const PROBABILITY_COUNT: usize = 1984 + (0x300 << 3);
const PROBABILITY_START: usize = 1664;
const INITIAL_PROBABILITY: u16 = 1024;
const BAD_INITIAL_CODE: u32 = 0xbfff_fc00;
const MAX_MATCH_REMAINDER: u32 = 273;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

#[repr(C)]
struct Properties {
    lc: u8,
    lp: u8,
    pb: u8,
    pad: u8,
    dictionary_size: u32,
}

#[repr(C)]
struct Decoder {
    properties: Properties,
    probabilities: *mut u16,
    probabilities_1664: *mut u16,
    dictionary: *mut u8,
    dictionary_size: usize,
    dictionary_position: usize,
    input: *const u8,
    range: u32,
    code: u32,
    processed_position: u32,
    checked_dictionary_size: u32,
    repetitions: [u32; 4],
    state: u32,
    remaining_length: u32,
    probability_count: u32,
    temporary_size: u32,
    temporary: [u8; 20],
}

const _: () = {
    assert!(size_of::<Properties>() == 8);
    assert!(offset_of!(Decoder, probabilities) == 8);
    assert!(offset_of!(Decoder, probabilities_1664) == 16);
    assert!(offset_of!(Decoder, dictionary) == 24);
    assert!(offset_of!(Decoder, dictionary_size) == 32);
    assert!(offset_of!(Decoder, dictionary_position) == 40);
    assert!(offset_of!(Decoder, input) == 48);
    assert!(offset_of!(Decoder, range) == 56);
    assert!(offset_of!(Decoder, code) == 60);
    assert!(offset_of!(Decoder, processed_position) == 64);
    assert!(offset_of!(Decoder, checked_dictionary_size) == 68);
    assert!(offset_of!(Decoder, repetitions) == 72);
    assert!(offset_of!(Decoder, state) == 88);
    assert!(offset_of!(Decoder, remaining_length) == 92);
    assert!(size_of::<Decoder>() == 128);
};

/// Decodes one exact raw-LZMA1 range into its final disjoint output range.
///
/// `input` must have at least 21 additional readable bytes after the slice. The
/// caller supplies that padding in the payload mapping. The expected trailing
/// count is part of the authenticated codec-4 chunk table.
pub fn decompress(
    input: &[u8],
    output: &mut [u8],
    properties: [u8; 5],
    expected_trailing: u8,
) -> Result<(), DecodeError> {
    if input.len() < 5
        || output.is_empty()
        || input[0] != 0
        || properties[0] != 0x5d
        || expected_trailing > 5
    {
        return Err(DecodeError);
    }
    let code = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
    if code >= BAD_INITIAL_CODE {
        return Err(DecodeError);
    }
    let dictionary_size =
        u32::from_le_bytes([properties[1], properties[2], properties[3], properties[4]]);
    let mut probabilities = [INITIAL_PROBABILITY; PROBABILITY_COUNT];
    let mut decoder = Decoder {
        properties: Properties {
            lc: 3,
            lp: 0,
            pb: 2,
            pad: 0,
            dictionary_size,
        },
        probabilities: probabilities.as_mut_ptr(),
        probabilities_1664: unsafe { probabilities.as_mut_ptr().add(PROBABILITY_START) },
        dictionary: output.as_mut_ptr(),
        dictionary_size: output.len(),
        dictionary_position: 0,
        input: unsafe { input.as_ptr().add(5) },
        range: u32::MAX,
        code,
        processed_position: 0,
        checked_dictionary_size: 0,
        repetitions: [1; 4],
        state: 0,
        remaining_length: 0,
        probability_count: PROBABILITY_COUNT as u32,
        temporary_size: 0,
        temporary: [0; 20],
    };
    let result =
        unsafe { decode_real(&mut decoder, output.len(), input.as_ptr().add(input.len())) };
    if result != 0 {
        return Err(DecodeError);
    }
    finish_remaining(&mut decoder, output.len())?;
    if decoder.dictionary_position < output.len() && decoder.remaining_length == 0 {
        let padding = [0u8; 21];
        decoder.input = padding.as_ptr();
        let result = unsafe { decode_real(&mut decoder, output.len(), padding.as_ptr().add(1)) };
        if result != 0 {
            return Err(DecodeError);
        }
        finish_remaining(&mut decoder, output.len())?;
    }
    if decoder.dictionary_position != output.len() {
        return Err(DecodeError);
    }
    if decoder.state >= 12 {
        return Err(DecodeError);
    }
    if decoder.remaining_length > MAX_MATCH_REMAINDER {
        return Err(DecodeError);
    }
    Ok(())
}

fn finish_remaining(decoder: &mut Decoder, output_limit: usize) -> Result<(), DecodeError> {
    let result: i32;
    unsafe {
        asm!(
            "call packforge_lzma_finish_remaining",
            in("rdi") decoder,
            in("rsi") output_limit,
            lateout("eax") result,
            clobber_abi("C"),
        );
    }
    if result == 0 {
        Ok(())
    } else {
        Err(DecodeError)
    }
}

#[inline(never)]
unsafe fn decode_real(decoder: &mut Decoder, output_limit: usize, input_limit: *const u8) -> i32 {
    let result: i32;
    unsafe {
        asm!(
            "call LzmaDec_DecodeReal_3",
            in("rdi") decoder,
            in("rsi") output_limit,
            in("rdx") input_limit,
            lateout("eax") result,
            clobber_abi("C"),
        );
    }
    result
}
