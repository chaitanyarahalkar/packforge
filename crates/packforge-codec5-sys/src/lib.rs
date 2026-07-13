//! Safe host wrappers around the pinned APultra and 7-Zip BCJ2 encoders.

use std::ffi::{c_int, c_uint};

const STREAM_COUNT: usize = 4;

unsafe extern "C" {
    fn apultra_get_max_compressed_size(input_size: usize) -> usize;
    fn apultra_compress(
        input: *const u8,
        output: *mut u8,
        input_size: usize,
        output_size: usize,
        flags: c_uint,
        maximum_window: usize,
        dictionary_size: usize,
        progress: *const (),
        statistics: *mut (),
    ) -> usize;
    fn apultra_decompress(
        input: *const u8,
        output: *mut u8,
        input_size: usize,
        output_size: usize,
        dictionary_size: usize,
        flags: c_uint,
    ) -> usize;
    fn packforge_bcj2_encode(
        input: *const u8,
        input_length: usize,
        outputs: *mut *mut u8,
        capacities: *const usize,
        lengths: *mut usize,
    ) -> c_int;
    fn packforge_bcj2_decode(
        inputs: *const *const u8,
        lengths: *const usize,
        output: *mut u8,
        output_length: usize,
    ) -> c_int;
}

/// A checked host codec failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error;

/// Compresses one non-empty byte range with pinned APultra.
pub fn apultra_compress_bytes(input: &[u8]) -> Result<Vec<u8>, Error> {
    if input.is_empty() {
        return Err(Error);
    }
    let capacity = unsafe { apultra_get_max_compressed_size(input.len()) };
    if capacity == usize::MAX {
        return Err(Error);
    }
    let mut output = vec![0u8; capacity];
    let length = unsafe {
        apultra_compress(
            input.as_ptr(),
            output.as_mut_ptr(),
            input.len(),
            capacity,
            0,
            2_097_152,
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if length == usize::MAX || length > capacity {
        return Err(Error);
    }
    output.truncate(length);
    Ok(output)
}

/// Decompresses one APultra range to its exact declared size.
pub fn apultra_decompress_bytes(input: &[u8], output_length: usize) -> Result<Vec<u8>, Error> {
    if input.is_empty() || output_length == 0 {
        return Err(Error);
    }
    let mut output = vec![0u8; output_length];
    let length = unsafe {
        apultra_decompress(
            input.as_ptr(),
            output.as_mut_ptr(),
            input.len(),
            output_length,
            0,
            0,
        )
    };
    if length != output_length {
        return Err(Error);
    }
    Ok(output)
}

/// Splits one runtime image into the four canonical BCJ2 streams.
pub fn bcj2_encode(input: &[u8]) -> Result<[Vec<u8>; STREAM_COUNT], Error> {
    if input.is_empty() {
        return Err(Error);
    }
    let capacity = input.len().checked_add(16).ok_or(Error)?;
    let mut streams: [Vec<u8>; STREAM_COUNT] = std::array::from_fn(|_| vec![0u8; capacity]);
    let mut pointers = streams.each_mut().map(Vec::as_mut_ptr);
    let capacities = [capacity; STREAM_COUNT];
    let mut lengths = [0usize; STREAM_COUNT];
    let result = unsafe {
        packforge_bcj2_encode(
            input.as_ptr(),
            input.len(),
            pointers.as_mut_ptr(),
            capacities.as_ptr(),
            lengths.as_mut_ptr(),
        )
    };
    if result != 0 {
        return Err(Error);
    }
    for (stream, length) in streams.iter_mut().zip(lengths) {
        if length > capacity {
            return Err(Error);
        }
        stream.truncate(length);
    }
    Ok(streams)
}

/// Reconstructs one exact runtime image from four BCJ2 streams.
pub fn bcj2_decode(streams: [&[u8]; STREAM_COUNT], output_length: usize) -> Result<Vec<u8>, Error> {
    let pointers = streams.map(<[u8]>::as_ptr);
    let lengths = streams.map(<[u8]>::len);
    let mut output = vec![0u8; output_length];
    let result = unsafe {
        packforge_bcj2_decode(
            pointers.as_ptr(),
            lengths.as_ptr(),
            output.as_mut_ptr(),
            output_length,
        )
    };
    if result != 0 {
        return Err(Error);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{apultra_compress_bytes, apultra_decompress_bytes, bcj2_decode, bcj2_encode};

    #[test]
    fn apultra_and_bcj2_round_trip() {
        let input: Vec<u8> = (0..65_537)
            .map(|index| (index as u8).wrapping_mul(17).wrapping_add(3))
            .collect();
        let compressed = apultra_compress_bytes(&input).unwrap();
        assert_eq!(
            apultra_decompress_bytes(&compressed, input.len()).unwrap(),
            input
        );
        let streams = bcj2_encode(&input).unwrap();
        assert_eq!(
            bcj2_decode(
                [&streams[0], &streams[1], &streams[2], &streams[3]],
                input.len()
            )
            .unwrap(),
            input
        );
    }
}
