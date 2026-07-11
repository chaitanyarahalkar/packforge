//! Formatting for the trusted procfs path used by the diskless exec fallback.

/// Writes a NUL-terminated `/proc/self/fd/<fd>` path into a fixed buffer.
pub fn format(fd: usize, output: &mut [u8; 40]) {
    const PREFIX: &[u8] = b"/proc/self/fd/";
    output.fill(0);
    output[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut reversed = [0u8; 20];
    let mut value = fd;
    let mut digits = 0usize;
    loop {
        reversed[digits] = b'0' + (value % 10) as u8;
        digits += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for index in 0..digits {
        output[PREFIX.len() + index] = reversed[digits - index - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    #[test]
    fn formats_zero_small_and_maximum_descriptors() {
        for descriptor in [0, 3, usize::MAX] {
            let mut output = [0xa5; 40];
            format(descriptor, &mut output);
            let end = output.iter().position(|byte| *byte == 0).unwrap();
            let actual = core::str::from_utf8(&output[..end]).unwrap();
            assert_eq!(actual, std::format!("/proc/self/fd/{descriptor}"));
            assert!(output[end..].iter().all(|byte| *byte == 0));
        }
    }
}
