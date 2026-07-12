// Compact unkeyed BLAKE3 used by the freestanding runtime.

const BLOCK_LEN: usize = 64;
const CHUNK_LEN: usize = 1024;
const CHUNK_START: u32 = 1;
const CHUNK_END: u32 = 2;
const PARENT: u32 = 4;
const ROOT: u32 = 8;

const IV: [u32; 8] = [
    0x6A09_E667,
    0xBB67_AE85,
    0x3C6E_F372,
    0xA54F_F53A,
    0x510E_527F,
    0x9B05_688C,
    0x1F83_D9AB,
    0x5BE0_CD19,
];

const MSG_SCHEDULE: [[u8; 16]; 7] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8],
    [3, 4, 10, 12, 13, 2, 7, 14, 6, 5, 9, 0, 11, 15, 8, 1],
    [10, 7, 12, 9, 14, 3, 13, 15, 4, 0, 11, 2, 5, 8, 1, 6],
    [12, 13, 9, 11, 15, 10, 14, 8, 7, 2, 5, 3, 0, 1, 6, 4],
    [9, 14, 11, 5, 8, 12, 15, 1, 13, 3, 0, 10, 2, 6, 4, 7],
    [11, 15, 5, 0, 1, 9, 8, 6, 14, 10, 2, 12, 3, 4, 7, 13],
];

#[derive(Clone, Copy)]
struct Output {
    input_cv: [u32; 8],
    block_words: [u32; 16],
    counter: u64,
    block_len: u32,
    flags: u32,
}

impl Output {
    fn chaining_value(self) -> [u32; 8] {
        let words = compress(
            self.input_cv,
            self.block_words,
            self.counter,
            self.block_len,
            self.flags,
        );
        words[..8].try_into().expect("fixed compression output")
    }

    fn root_hash(self) -> [u8; 32] {
        let words = compress(
            self.input_cv,
            self.block_words,
            self.counter,
            self.block_len,
            self.flags | ROOT,
        );
        let mut output = [0u8; 32];
        for (destination, word) in output.chunks_exact_mut(4).zip(words) {
            destination.copy_from_slice(&word.to_le_bytes());
        }
        output
    }
}

/// Computes the 32-byte unkeyed BLAKE3 digest of an in-memory input.
#[must_use]
pub fn hash(input: &[u8]) -> [u8; 32] {
    let chunk_count = input.len().div_ceil(CHUNK_LEN).max(1);
    let last_chunk = chunk_count - 1;
    let mut cv_stack = [[0u32; 8]; 54];
    let mut stack_len = 0usize;

    for chunk_index in 0..last_chunk {
        let start = chunk_index * CHUNK_LEN;
        let output = chunk_output(&input[start..start + CHUNK_LEN], chunk_index as u64);
        let mut cv = output.chaining_value();
        let mut total_chunks = chunk_index + 1;
        while total_chunks & 1 == 0 {
            stack_len -= 1;
            cv = parent_output(cv_stack[stack_len], cv).chaining_value();
            total_chunks >>= 1;
        }
        cv_stack[stack_len] = cv;
        stack_len += 1;
    }

    let last_start = last_chunk * CHUNK_LEN;
    let mut output = chunk_output(&input[last_start..], last_chunk as u64);
    while stack_len > 0 {
        stack_len -= 1;
        output = parent_output(cv_stack[stack_len], output.chaining_value());
    }
    output.root_hash()
}

fn chunk_output(chunk: &[u8], chunk_counter: u64) -> Output {
    let block_count = chunk.len().div_ceil(BLOCK_LEN).max(1);
    let mut cv = IV;
    for block_index in 0..block_count - 1 {
        let start = block_index * BLOCK_LEN;
        let block_words = words_from_block(&chunk[start..start + BLOCK_LEN]);
        let flags = if block_index == 0 { CHUNK_START } else { 0 };
        let compressed = compress(cv, block_words, chunk_counter, BLOCK_LEN as u32, flags);
        cv.copy_from_slice(&compressed[..8]);
    }

    let last_index = block_count - 1;
    let last_start = last_index * BLOCK_LEN;
    let last_block = &chunk[last_start..];
    let flags = CHUNK_END | if last_index == 0 { CHUNK_START } else { 0 };
    Output {
        input_cv: cv,
        block_words: words_from_block(last_block),
        counter: chunk_counter,
        block_len: last_block.len() as u32,
        flags,
    }
}

fn parent_output(left: [u32; 8], right: [u32; 8]) -> Output {
    let mut block_words = [0u32; 16];
    block_words[..8].copy_from_slice(&left);
    block_words[8..].copy_from_slice(&right);
    Output {
        input_cv: IV,
        block_words,
        counter: 0,
        block_len: BLOCK_LEN as u32,
        flags: PARENT,
    }
}

#[cfg(feature = "lzma")]
fn words_from_block(block: &[u8]) -> [u32; 16] {
    let mut words = [0u32; 16];
    for (word_index, word) in words.iter_mut().enumerate() {
        let byte_index = word_index * 4;
        let mut bytes = [0u8; 4];
        for (offset, byte) in bytes.iter_mut().enumerate() {
            *byte = block.get(byte_index + offset).copied().unwrap_or(0);
        }
        *word = u32::from_le_bytes(bytes);
    }
    words
}

#[cfg(not(feature = "lzma"))]
fn words_from_block(block: &[u8]) -> [u32; 16] {
    let mut padded = [0u8; BLOCK_LEN];
    padded[..block.len()].copy_from_slice(block);
    let mut words = [0u32; 16];
    for (word, bytes) in words.iter_mut().zip(padded.chunks_exact(4)) {
        *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    words
}

fn compress(
    chaining_value: [u32; 8],
    block_words: [u32; 16],
    counter: u64,
    block_len: u32,
    flags: u32,
) -> [u32; 16] {
    let mut state = [0u32; 16];
    state[..8].copy_from_slice(&chaining_value);
    state[8..12].copy_from_slice(&IV[..4]);
    state[12] = counter as u32;
    state[13] = (counter >> 32) as u32;
    state[14] = block_len;
    state[15] = flags;

    #[cfg(not(feature = "optimized"))]
    for schedule in MSG_SCHEDULE {
        round(&mut state, &block_words, &schedule);
    }
    #[cfg(feature = "optimized")]
    for schedule in &MSG_SCHEDULE {
        round(&mut state, &block_words, schedule);
    }
    for index in 0..8 {
        state[index] ^= state[index + 8];
        state[index + 8] ^= chaining_value[index];
    }
    state
}

fn round(state: &mut [u32; 16], message: &[u32; 16], schedule: &[u8; 16]) {
    g(
        state,
        0,
        4,
        8,
        12,
        message[usize::from(schedule[0])],
        message[usize::from(schedule[1])],
    );
    g(
        state,
        1,
        5,
        9,
        13,
        message[usize::from(schedule[2])],
        message[usize::from(schedule[3])],
    );
    g(
        state,
        2,
        6,
        10,
        14,
        message[usize::from(schedule[4])],
        message[usize::from(schedule[5])],
    );
    g(
        state,
        3,
        7,
        11,
        15,
        message[usize::from(schedule[6])],
        message[usize::from(schedule[7])],
    );
    g(
        state,
        0,
        5,
        10,
        15,
        message[usize::from(schedule[8])],
        message[usize::from(schedule[9])],
    );
    g(
        state,
        1,
        6,
        11,
        12,
        message[usize::from(schedule[10])],
        message[usize::from(schedule[11])],
    );
    g(
        state,
        2,
        7,
        8,
        13,
        message[usize::from(schedule[12])],
        message[usize::from(schedule[13])],
    );
    g(
        state,
        3,
        4,
        9,
        14,
        message[usize::from(schedule[14])],
        message[usize::from(schedule[15])],
    );
}

#[allow(clippy::many_single_char_names)]
fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, first: u32, second: u32) {
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(first);
    state[d] = (state[d] ^ state[a]).rotate_right(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(12);
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(second);
    state[d] = (state[d] ^ state[a]).rotate_right(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(7);
}

#[cfg(test)]
mod tests {
    use super::hash;

    #[test]
    fn matches_reference_across_block_chunk_and_tree_boundaries() {
        for length in [0, 1, 63, 64, 65, 1023, 1024, 1025, 2048, 3073, 16_384] {
            let input: std::vec::Vec<u8> = (0..length)
                .map(|index| (index as u8).wrapping_mul(37).wrapping_add(11))
                .collect();
            assert_eq!(hash(&input), *blake3::hash(&input).as_bytes(), "{length}");
        }
    }
}
