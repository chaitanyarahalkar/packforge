//! Audited baseline-SSE2 four-lane BLAKE3 compression for the x86-64 runtime.

#![no_std]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_ptr_alignment
)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m128i, _mm_add_epi32, _mm_or_si128, _mm_set1_epi32, _mm_setr_epi32, _mm_slli_epi32,
    _mm_srli_epi32, _mm_storeu_si128, _mm_xor_si128,
};

/// Compresses four independent BLAKE3 blocks with baseline x86-64 SSE2.
///
/// The four lanes use consecutive counters beginning at `first_counter`.
#[cfg(target_arch = "x86_64")]
#[must_use]
#[allow(unsafe_code)]
pub fn compress4(
    chaining_values: [[u32; 8]; 4],
    block_words: [[u32; 16]; 4],
    first_counter: u64,
    block_len: u32,
    flags: u32,
    iv: [u32; 8],
    schedule: &[[u8; 16]; 7],
) -> [[u32; 8]; 4] {
    // SAFETY: SSE2 is mandatory for x86-64. Every intrinsic load and store
    // addresses a complete local fixed-size array, and schedule entries are
    // the caller's canonical BLAKE3 constants in the range 0..16.
    unsafe {
        compress4_sse2(
            chaining_values,
            block_words,
            first_counter,
            block_len,
            flags,
            iv,
            schedule,
        )
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(unsafe_code)]
unsafe fn compress4_sse2(
    chaining_values: [[u32; 8]; 4],
    block_words: [[u32; 16]; 4],
    first_counter: u64,
    block_len: u32,
    flags: u32,
    iv: [u32; 8],
    schedule: &[[u8; 16]; 7],
) -> [[u32; 8]; 4] {
    let mut state = [_mm_set1_epi32(0); 16];
    let mut message = [_mm_set1_epi32(0); 16];
    for word in 0..8 {
        state[word] = lane(
            chaining_values[0][word],
            chaining_values[1][word],
            chaining_values[2][word],
            chaining_values[3][word],
        );
    }
    for word in 0..4 {
        state[word + 8] = _mm_set1_epi32(iv[word] as i32);
    }
    state[12] = lane(
        first_counter as u32,
        first_counter.wrapping_add(1) as u32,
        first_counter.wrapping_add(2) as u32,
        first_counter.wrapping_add(3) as u32,
    );
    state[13] = lane(
        (first_counter >> 32) as u32,
        (first_counter.wrapping_add(1) >> 32) as u32,
        (first_counter.wrapping_add(2) >> 32) as u32,
        (first_counter.wrapping_add(3) >> 32) as u32,
    );
    state[14] = _mm_set1_epi32(block_len as i32);
    state[15] = _mm_set1_epi32(flags as i32);
    for word in 0..16 {
        message[word] = lane(
            block_words[0][word],
            block_words[1][word],
            block_words[2][word],
            block_words[3][word],
        );
    }
    for round_schedule in schedule {
        round(&mut state, &message, round_schedule);
    }
    let mut output = [[0u32; 8]; 4];
    for word in 0..8 {
        store_lanes(
            _mm_xor_si128(state[word], state[word + 8]),
            &mut output,
            word,
        );
    }
    output
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn lane(a: u32, b: u32, c: u32, d: u32) -> __m128i {
    _mm_setr_epi32(a as i32, b as i32, c as i32, d as i32)
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn store_lanes(value: __m128i, output: &mut [[u32; 8]; 4], word: usize) {
    let mut lanes = [0u32; 4];
    _mm_storeu_si128(lanes.as_mut_ptr().cast::<__m128i>(), value);
    for lane_index in 0..4 {
        output[lane_index][word] = lanes[lane_index];
    }
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn round(state: &mut [__m128i; 16], message: &[__m128i; 16], schedule: &[u8; 16]) {
    g(state, 0, 4, 8, 12, message, schedule[0], schedule[1]);
    g(state, 1, 5, 9, 13, message, schedule[2], schedule[3]);
    g(state, 2, 6, 10, 14, message, schedule[4], schedule[5]);
    g(state, 3, 7, 11, 15, message, schedule[6], schedule[7]);
    g(state, 0, 5, 10, 15, message, schedule[8], schedule[9]);
    g(state, 1, 6, 11, 12, message, schedule[10], schedule[11]);
    g(state, 2, 7, 8, 13, message, schedule[12], schedule[13]);
    g(state, 3, 4, 9, 14, message, schedule[14], schedule[15]);
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code, clippy::too_many_arguments)]
unsafe fn g(
    state: &mut [__m128i; 16],
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    message: &[__m128i; 16],
    first: u8,
    second: u8,
) {
    let mut av = state[a];
    let mut bv = state[b];
    let mut cv = state[c];
    let mut dv = state[d];
    av = _mm_add_epi32(_mm_add_epi32(av, bv), message[usize::from(first)]);
    dv = rotate16(_mm_xor_si128(dv, av));
    cv = _mm_add_epi32(cv, dv);
    bv = rotate12(_mm_xor_si128(bv, cv));
    av = _mm_add_epi32(_mm_add_epi32(av, bv), message[usize::from(second)]);
    dv = rotate8(_mm_xor_si128(dv, av));
    cv = _mm_add_epi32(cv, dv);
    bv = rotate7(_mm_xor_si128(bv, cv));
    state[a] = av;
    state[b] = bv;
    state[c] = cv;
    state[d] = dv;
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn rotate16(value: __m128i) -> __m128i {
    _mm_or_si128(_mm_srli_epi32::<16>(value), _mm_slli_epi32::<16>(value))
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn rotate12(value: __m128i) -> __m128i {
    _mm_or_si128(_mm_srli_epi32::<12>(value), _mm_slli_epi32::<20>(value))
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn rotate8(value: __m128i) -> __m128i {
    _mm_or_si128(_mm_srli_epi32::<8>(value), _mm_slli_epi32::<24>(value))
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
unsafe fn rotate7(value: __m128i) -> __m128i {
    _mm_or_si128(_mm_srli_epi32::<7>(value), _mm_slli_epi32::<25>(value))
}
