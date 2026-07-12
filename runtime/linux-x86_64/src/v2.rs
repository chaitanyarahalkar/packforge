#![no_main]
#![no_std]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::{ptr, slice};

use packforge_runtime_linux_x86_64::hash;
use packforge_runtime_linux_x86_64::lzma;
use packforge_runtime_linux_x86_64::v2_format::{
    self, ElfInfo, HEADER_LEN, MANIFEST_HEADER_LEN, MANIFEST_SEGMENT_LEN, MAX_SEGMENTS, Manifest,
    OutputLayout, Segment, TRAILER_LEN,
};

const MAX_MANIFEST_LENGTH: usize = MANIFEST_HEADER_LEN + MAX_SEGMENTS * MANIFEST_SEGMENT_LEN;

const SYS_WRITE: usize = 1;
const SYS_CLOSE: usize = 3;
const SYS_LSEEK: usize = 8;
const SYS_MMAP: usize = 9;
const SYS_MPROTECT: usize = 10;
const SYS_MUNMAP: usize = 11;
const SYS_PREAD64: usize = 17;
const SYS_EXIT_GROUP: usize = 231;
const SYS_OPENAT: usize = 257;

const AT_FDCWD: isize = -100;
const O_RDONLY: usize = 0;
const O_CLOEXEC: usize = 0x80000;
const SEEK_END: usize = 2;
const PROT_READ: usize = 1;
const PROT_WRITE: usize = 2;
const PROT_EXEC: usize = 4;
const MAP_PRIVATE: usize = 2;
const MAP_FIXED_NOREPLACE: usize = 0x10_0000;
const MAP_ANONYMOUS: usize = 0x20;

const AT_NULL: usize = 0;
const AT_PHDR: usize = 3;
const AT_PHENT: usize = 4;
const AT_PHNUM: usize = 5;
const AT_BASE: usize = 7;
const AT_ENTRY: usize = 9;
const AUX_REQUIRED: u8 = 0x0f;

#[unsafe(link_section = ".rodata.packforge.000_panic")]
static PANIC_MESSAGE: [u8; 28] = *b"packforge: v2 runtime panic\n";

global_asm!(
    r#"
    .hidden memcpy
    .hidden memmove
    .hidden memset
    .hidden bcmp
    .global memcpy
    .type memcpy,@function
memcpy:
    mov %rdi, %rax
    xor %rcx, %rcx
.Lmemcpy_loop:
    cmp %rdx, %rcx
    je .Lmemcpy_done
    movb (%rsi,%rcx), %r8b
    movb %r8b, (%rdi,%rcx)
    inc %rcx
    jmp .Lmemcpy_loop
.Lmemcpy_done:
    ret
    .size memcpy, .-memcpy
    .global _start
    .type _start,@function
_start:
    xor %rbp, %rbp
    mov %rsp, %rdi
    mov %rdx, %rsi
    and $-16, %rsp
    call runtime_main
    ud2
    .size _start, .-_start
"#,
    options(att_syntax)
);

#[panic_handler]
#[cold]
#[inline(never)]
#[unsafe(link_section = ".text.packforge.000_panic")]
fn panic(_info: &PanicInfo<'_>) -> ! {
    fail(&PANIC_MESSAGE)
}

#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

#[unsafe(no_mangle)]
unsafe extern "C" fn runtime_main(stack: *mut usize, rtld_fini: usize) -> ! {
    match unsafe { run(stack, rtld_fini) } {
        Ok(never) => match never {},
        Err(message) => fail(message),
    }
}

unsafe fn run(
    stack: *mut usize,
    rtld_fini: usize,
) -> Result<core::convert::Infallible, &'static [u8]> {
    let self_fd = syscall4(
        SYS_OPENAT,
        AT_FDCWD as usize,
        c"/proc/self/exe".as_ptr() as usize,
        O_RDONLY | O_CLOEXEC,
        0,
    );
    if is_error(self_fd) {
        return Err(b"packforge: cannot open /proc/self/exe\n");
    }
    let self_fd = self_fd as usize;
    let file_length = syscall3(SYS_LSEEK, self_fd, 0, SEEK_END);
    if is_error(file_length) || file_length < TRAILER_LEN as isize {
        return Err(b"packforge: invalid v2 executable length\n");
    }
    let file_length = file_length as u64;

    let mut trailer_bytes = [0u8; TRAILER_LEN];
    let trailer_offset = file_length
        .checked_sub(TRAILER_LEN as u64)
        .ok_or(b"packforge: invalid v2 trailer\n" as &'static [u8])?;
    pread_exact(self_fd, &mut trailer_bytes, trailer_offset)
        .map_err(|()| b"packforge: cannot read v2 trailer\n" as &'static [u8])?;
    let trailer =
        v2_format::parse_trailer(&trailer_bytes, file_length).map_err(format_error_message)?;

    verify_file_range(
        self_fd,
        0,
        trailer.loader_length,
        trailer.loader_digest,
        b"packforge: v2 loader integrity failed\n",
    )?;

    let mut header_bytes = [0u8; HEADER_LEN];
    pread_exact(self_fd, &mut header_bytes, trailer.image_offset)
        .map_err(|()| b"packforge: cannot read v2 image header\n" as &'static [u8])?;
    let header = v2_format::parse_header(&header_bytes).map_err(format_error_message)?;
    v2_format::validate_image_layout(&trailer, &header).map_err(format_error_message)?;

    let manifest_length = usize::try_from(header.manifest_length)
        .map_err(|_| b"packforge: v2 manifest is too large\n" as &'static [u8])?;
    let mut manifest_storage = [0u8; MAX_MANIFEST_LENGTH];
    let manifest_bytes = manifest_storage
        .get_mut(..manifest_length)
        .ok_or(b"packforge: v2 manifest is too large\n" as &'static [u8])?;
    let manifest_offset = trailer
        .image_offset
        .checked_add(HEADER_LEN as u64)
        .ok_or(b"packforge: invalid v2 image layout\n" as &'static [u8])?;
    pread_exact(self_fd, manifest_bytes, manifest_offset)
        .map_err(|()| b"packforge: cannot read v2 manifest\n" as &'static [u8])?;
    if hash::hash(manifest_bytes) != header.manifest_digest {
        return Err(b"packforge: v2 manifest integrity failed\n");
    }
    let manifest = v2_format::parse_manifest(manifest_bytes, header.original_length)
        .map_err(format_error_message)?;

    let payload_length = usize::try_from(header.payload_length)
        .map_err(|_| b"packforge: v2 payload is too large\n" as &'static [u8])?;
    let payload_mapping = map_writable(payload_length)
        .ok_or(b"packforge: cannot allocate v2 payload\n" as &'static [u8])?;
    let payload = unsafe { slice::from_raw_parts_mut(payload_mapping, payload_length) };
    let payload_offset = manifest_offset
        .checked_add(header.manifest_length)
        .ok_or(b"packforge: invalid v2 payload offset\n" as &'static [u8])?;
    pread_exact(self_fd, payload, payload_offset)
        .map_err(|()| b"packforge: cannot read v2 payload\n" as &'static [u8])?;
    if hash::hash(payload) != header.payload_digest {
        return Err(b"packforge: v2 payload integrity failed\n");
    }

    let original_length = usize::try_from(header.original_length)
        .map_err(|_| b"packforge: v2 original is too large\n" as &'static [u8])?;
    let output_layout = v2_format::direct_output_layout(&manifest).map_err(format_error_message)?;
    let original_mapping = reserve_output(output_layout)?;
    let original = unsafe { slice::from_raw_parts_mut(original_mapping, original_length) };
    let report = lzma::decompress(payload, &header.properties, original)
        .map_err(|_| b"packforge: v2 LZMA1 decompression failed\n" as &'static [u8])?;
    if report.trailing_bytes != header.trailing_bytes {
        return Err(b"packforge: v2 LZMA1 framing failed\n");
    }
    if hash::hash(original) != header.original_digest {
        return Err(b"packforge: v2 original integrity failed\n");
    }
    let elf = v2_format::validate_elf(original, &manifest).map_err(format_error_message)?;

    finalize_output(output_layout, &manifest)?;
    unsafe { rewrite_auxiliary_vector(stack, elf) }?;

    let _ = syscall1(SYS_CLOSE, self_fd);
    let _ = syscall2(SYS_MUNMAP, payload_mapping as usize, payload_length);
    unsafe { transfer(stack, elf.entry_point as usize, rtld_fini) }
}

fn verify_file_range(
    fd: usize,
    offset: u64,
    length: u64,
    expected: [u8; 32],
    message: &'static [u8],
) -> Result<(), &'static [u8]> {
    let length = usize::try_from(length).map_err(|_| message)?;
    let mapping = map_writable(length).ok_or(message)?;
    let bytes = unsafe { slice::from_raw_parts_mut(mapping, length) };
    let result = pread_exact(fd, bytes, offset)
        .map_err(|()| message)
        .and_then(|()| {
            if hash::hash(bytes) == expected {
                Ok(())
            } else {
                Err(message)
            }
        });
    let _ = syscall2(SYS_MUNMAP, mapping as usize, length);
    result
}

fn reserve_output(layout: OutputLayout) -> Result<*mut u8, &'static [u8]> {
    let address = usize::try_from(layout.start)
        .map_err(|_| b"packforge: target mapping is out of range\n" as &'static [u8])?;
    let length = usize::try_from(layout.length)
        .map_err(|_| b"packforge: target mapping is out of range\n" as &'static [u8])?;
    let mapped = syscall6(
        SYS_MMAP,
        address,
        length,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE,
        usize::MAX,
        0,
    );
    if is_error(mapped) || mapped as usize != address {
        if !is_error(mapped) {
            let _ = syscall2(SYS_MUNMAP, mapped as usize, length);
        }
        return Err(b"packforge: target address collision\n");
    }
    Ok(mapped as *mut u8)
}

fn finalize_output(layout: OutputLayout, manifest: &Manifest) -> Result<(), &'static [u8]> {
    for segment in manifest.segments.iter().take(manifest.count) {
        let zero_start = segment
            .virtual_address
            .checked_add(segment.file_size)
            .ok_or(b"packforge: target address is out of range\n" as &'static [u8])?;
        let zero_length = usize::try_from(segment.memory_size - segment.file_size)
            .map_err(|_| b"packforge: target mapping is out of range\n" as &'static [u8])?;
        for index in 0..zero_length {
            unsafe { ptr::write_volatile((zero_start as *mut u8).add(index), 0) };
        }
    }

    let span_end = layout
        .start
        .checked_add(layout.length)
        .ok_or(b"packforge: target mapping is out of range\n" as &'static [u8])?;
    let mut cursor = layout.start;
    for _ in 0..manifest.count {
        let segment = manifest
            .segments
            .iter()
            .take(manifest.count)
            .filter(|segment| segment.map_start >= cursor)
            .min_by_key(|segment| segment.map_start)
            .ok_or(b"packforge: invalid target mapping order\n" as &'static [u8])?;
        if segment.map_start > cursor {
            let _ = syscall2(
                SYS_MUNMAP,
                cursor as usize,
                (segment.map_start - cursor) as usize,
            );
        }
        if is_error(syscall3(
            SYS_MPROTECT,
            segment.map_start as usize,
            segment.map_length as usize,
            segment_protection(*segment),
        )) {
            return Err(b"packforge: cannot apply target protections\n");
        }
        cursor = segment
            .map_start
            .checked_add(segment.map_length)
            .ok_or(b"packforge: target mapping is out of range\n" as &'static [u8])?;
    }
    if cursor < span_end {
        let _ = syscall2(SYS_MUNMAP, cursor as usize, (span_end - cursor) as usize);
    }
    Ok(())
}

const fn segment_protection(segment: Segment) -> usize {
    let mut protection = 0usize;
    if segment.flags & 4 != 0 {
        protection |= PROT_READ;
    }
    if segment.flags & 2 != 0 {
        protection |= PROT_WRITE;
    }
    if segment.flags & 1 != 0 {
        protection |= PROT_EXEC;
    }
    protection
}

unsafe fn rewrite_auxiliary_vector(stack: *mut usize, elf: ElfInfo) -> Result<(), &'static [u8]> {
    let argc = unsafe { *stack };
    if argc > 1 << 20 {
        return Err(b"packforge: invalid initial stack\n");
    }
    let mut cursor = unsafe { stack.add(1 + argc) };
    if unsafe { *cursor } != 0 {
        return Err(b"packforge: invalid argv stack\n");
    }
    cursor = unsafe { cursor.add(1) };
    let mut environment_count = 0usize;
    while unsafe { *cursor } != 0 {
        environment_count += 1;
        if environment_count > 1 << 20 {
            return Err(b"packforge: invalid environment stack\n");
        }
        cursor = unsafe { cursor.add(1) };
    }
    cursor = unsafe { cursor.add(1) };
    let mut found = 0u8;
    for _ in 0..256 {
        let kind = unsafe { *cursor };
        if kind == AT_NULL {
            return if found == AUX_REQUIRED {
                Ok(())
            } else {
                Err(b"packforge: incomplete auxiliary vector\n")
            };
        }
        let value = unsafe { cursor.add(1) };
        match kind {
            AT_PHDR => {
                unsafe { *value = elf.program_header_address as usize };
                found |= 1;
            }
            AT_PHENT => {
                unsafe { *value = usize::from(elf.program_header_entry_size) };
                found |= 2;
            }
            AT_PHNUM => {
                unsafe { *value = usize::from(elf.program_header_count) };
                found |= 4;
            }
            AT_BASE => unsafe { *value = 0 },
            AT_ENTRY => {
                unsafe { *value = elf.entry_point as usize };
                found |= 8;
            }
            _ => {}
        }
        cursor = unsafe { cursor.add(2) };
    }
    Err(b"packforge: invalid auxiliary vector\n")
}

unsafe fn transfer(stack: *mut usize, entry: usize, rtld_fini: usize) -> ! {
    unsafe {
        asm!(
            "mov rsp, {initial_stack}",
            "xor ebp, ebp",
            "jmp {target_entry}",
            initial_stack = in(reg) stack,
            target_entry = in(reg) entry,
            in("rdx") rtld_fini,
            options(noreturn),
        );
    }
}

fn format_error_message(error: v2_format::Error) -> &'static [u8] {
    match error {
        v2_format::Error::Integrity => b"packforge: v2 metadata integrity failed\n",
        v2_format::Error::Overlap => b"packforge: overlapping target mappings\n",
        v2_format::Error::Permissions => b"packforge: writable executable target segment\n",
        v2_format::Error::Entry => b"packforge: invalid target entry point\n",
        v2_format::Error::ProgramHeaders => b"packforge: invalid target program headers\n",
        _ => b"packforge: invalid v2 metadata\n",
    }
}

#[inline(always)]
fn map_writable(length: usize) -> Option<*mut u8> {
    if length == 0 {
        return None;
    }
    let result = syscall6(
        SYS_MMAP,
        0,
        length,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS,
        usize::MAX,
        0,
    );
    (!is_error(result)).then_some(result as *mut u8)
}

#[inline(always)]
fn pread_exact(fd: usize, mut output: &mut [u8], mut offset: u64) -> Result<(), ()> {
    while !output.is_empty() {
        let result = syscall4(
            SYS_PREAD64,
            fd,
            output.as_mut_ptr() as usize,
            output.len(),
            offset as usize,
        );
        if result <= 0 || is_error(result) {
            return Err(());
        }
        let read = usize::try_from(result).map_err(|_| ())?;
        output = output.get_mut(read..).ok_or(())?;
        offset = offset.checked_add(read as u64).ok_or(())?;
    }
    Ok(())
}

#[inline(always)]
fn fail(message: &[u8]) -> ! {
    let _ = syscall3(SYS_WRITE, 2, message.as_ptr() as usize, message.len());
    let _ = syscall2(SYS_EXIT_GROUP, 127, 0);
    loop {
        core::hint::spin_loop();
    }
}

const fn is_error(result: isize) -> bool {
    result < 0 && result >= -4095
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memmove(destination: *mut u8, source: *const u8, count: usize) -> *mut u8 {
    if (destination as usize) <= (source as usize) {
        for index in 0..count {
            let byte = unsafe { ptr::read_volatile(source.add(index)) };
            unsafe { ptr::write_volatile(destination.add(index), byte) };
        }
    } else {
        for index in (0..count).rev() {
            let byte = unsafe { ptr::read_volatile(source.add(index)) };
            unsafe { ptr::write_volatile(destination.add(index), byte) };
        }
    }
    destination
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memset(destination: *mut u8, value: i32, count: usize) -> *mut u8 {
    for index in 0..count {
        unsafe { ptr::write_volatile(destination.add(index), value as u8) };
    }
    destination
}

#[unsafe(no_mangle)]
unsafe extern "C" fn bcmp(first: *const u8, second: *const u8, count: usize) -> i32 {
    for index in 0..count {
        if unsafe { *first.add(index) } != unsafe { *second.add(index) } {
            return 1;
        }
    }
    0
}

#[inline(always)]
fn syscall1(number: usize, first: usize) -> isize {
    syscall6(number, first, 0, 0, 0, 0, 0)
}

#[inline(always)]
fn syscall2(number: usize, first: usize, second: usize) -> isize {
    syscall6(number, first, second, 0, 0, 0, 0)
}

#[inline(always)]
fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    syscall6(number, first, second, third, 0, 0, 0)
}

#[inline(always)]
fn syscall4(number: usize, first: usize, second: usize, third: usize, fourth: usize) -> isize {
    syscall6(number, first, second, third, fourth, 0, 0)
}

#[inline(always)]
fn syscall6(
    number: usize,
    first: usize,
    second: usize,
    third: usize,
    fourth: usize,
    fifth: usize,
    sixth: usize,
) -> isize {
    let result: isize;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number as isize => result,
            in("rdi") first,
            in("rsi") second,
            in("rdx") third,
            in("r10") fourth,
            in("r8") fifth,
            in("r9") sixth,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    result
}
