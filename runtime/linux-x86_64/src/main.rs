#![no_main]
#![no_std]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr;
use core::slice;
use packforge_runtime_linux_x86_64::{hash, lz4, procfd};

const TRAILER_LEN: usize = 128;
const CONTAINER_HEADER_LEN: usize = 192;
const MAX_ORIGINAL_SIZE: u64 = 1 << 30;
const MAX_PAYLOAD_SIZE: u64 = MAX_ORIGINAL_SIZE + (64 << 20);
const MAX_STUB_SIZE: u64 = 32 * 1024;
const MAX_CONTAINER_SIZE: u64 = MAX_PAYLOAD_SIZE + CONTAINER_HEADER_LEN as u64;

const TRAILER_MAGIC: &[u8; 8] = b"PFGEXE01";
const CONTAINER_MAGIC: &[u8; 8] = b"PFGCNT01";
const TRAILER_HASH_OFFSET: usize = 96;
const HEADER_HASH_OFFSET: usize = 152;
const HEADER_HASH_END: usize = 184;

const SYS_WRITE: usize = 1;
const SYS_CLOSE: usize = 3;
const SYS_LSEEK: usize = 8;
const SYS_MMAP: usize = 9;
const SYS_MUNMAP: usize = 11;
const SYS_PREAD64: usize = 17;
const SYS_EXECVE: usize = 59;
const SYS_FCNTL: usize = 72;
const SYS_FCHMOD: usize = 91;
const SYS_EXIT_GROUP: usize = 231;
const SYS_OPENAT: usize = 257;
const SYS_MEMFD_CREATE: usize = 319;
const SYS_EXECVEAT: usize = 322;

const AT_FDCWD: isize = -100;
const AT_EMPTY_PATH: usize = 0x1000;
const O_RDONLY: usize = 0;
const O_CLOEXEC: usize = 0x80000;
const SEEK_END: usize = 2;
const PROT_READ: usize = 1;
const PROT_WRITE: usize = 2;
const MAP_PRIVATE: usize = 2;
const MAP_ANONYMOUS: usize = 0x20;
const MFD_CLOEXEC: usize = 1;
const MFD_ALLOW_SEALING: usize = 2;
const MFD_EXEC: usize = 0x10;
const F_ADD_SEALS: usize = 1033;
const F_SEAL_ALL: usize = 0x0f;
const EINVAL: isize = 22;

global_asm!(
    r#"
    .global _start
    .type _start,@function
_start:
    xor %rbp, %rbp
    mov %rsp, %rdi
    and $-16, %rsp
    call runtime_main
    ud2
    .size _start, .-_start
"#,
    options(att_syntax)
);

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    fail(b"packforge: runtime panic\n")
}

#[derive(Clone, Copy)]
struct Trailer {
    container_offset: u64,
    container_length: u64,
    executable_length: u64,
    loader_length: u64,
    loader_digest: [u8; 32],
}

struct Header {
    original_size: u64,
    payload_size: u64,
    original_digest: [u8; 32],
    payload_digest: [u8; 32],
}

#[unsafe(no_mangle)]
unsafe extern "C" fn runtime_main(stack: *const usize) -> ! {
    #[cfg(feature = "lzma-size-spike")]
    {
        let decoder: fn(
            &[u8],
            &[u8; 5],
            &mut [u8],
        ) -> Result<
            packforge_runtime_linux_x86_64::lzma::DecodeReport,
            packforge_runtime_linux_x86_64::lzma::DecodeError,
        > = packforge_runtime_linux_x86_64::lzma::decompress;
        core::hint::black_box(decoder);
    }
    let argc = unsafe { *stack };
    let argv = unsafe { stack.add(1) }.cast::<*const u8>();
    let envp = unsafe { argv.add(argc + 1) };
    match unsafe { run(argv, envp) } {
        Ok(never) => match never {},
        Err(message) => fail(message),
    }
}

unsafe fn run(
    argv: *const *const u8,
    envp: *const *const u8,
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
        return Err(b"packforge: invalid executable length\n");
    }
    let file_length = file_length as u64;

    let mut trailer_bytes = [0u8; TRAILER_LEN];
    let trailer_offset = file_length
        .checked_sub(TRAILER_LEN as u64)
        .ok_or(b"packforge: invalid executable trailer\n" as &'static [u8])?;
    pread_exact(self_fd, &mut trailer_bytes, trailer_offset)
        .map_err(|()| b"packforge: cannot read executable trailer\n" as &'static [u8])?;
    let trailer = parse_trailer(&trailer_bytes, file_length)?;

    let loader_length = usize::try_from(trailer.loader_length)
        .map_err(|_| b"packforge: loader is too large\n" as &'static [u8])?;
    let loader_mapping = map_writable(loader_length)
        .ok_or(b"packforge: cannot allocate loader memory\n" as &'static [u8])?;
    let loader = unsafe { slice::from_raw_parts_mut(loader_mapping, loader_length) };
    pread_exact(self_fd, loader, 0)
        .map_err(|()| b"packforge: cannot read runtime loader\n" as &'static [u8])?;
    if hash::hash(loader) != trailer.loader_digest {
        return Err(b"packforge: runtime loader integrity failed\n");
    }
    let _ = syscall2(SYS_MUNMAP, loader_mapping as usize, loader_length);

    let mut header_bytes = [0u8; CONTAINER_HEADER_LEN];
    pread_exact(self_fd, &mut header_bytes, trailer.container_offset)
        .map_err(|()| b"packforge: cannot read container header\n" as &'static [u8])?;
    let header = parse_header(&header_bytes, trailer.container_length)?;

    let payload_length = usize::try_from(header.payload_size)
        .map_err(|_| b"packforge: payload is too large\n" as &'static [u8])?;
    let original_length = usize::try_from(header.original_size)
        .map_err(|_| b"packforge: original is too large\n" as &'static [u8])?;
    let payload_mapping = map_writable(payload_length)
        .ok_or(b"packforge: cannot allocate payload memory\n" as &'static [u8])?;
    let payload = unsafe { slice::from_raw_parts_mut(payload_mapping, payload_length) };
    pread_exact(
        self_fd,
        payload,
        trailer.container_offset + CONTAINER_HEADER_LEN as u64,
    )
    .map_err(|()| b"packforge: cannot read compressed payload\n" as &'static [u8])?;
    if hash::hash(payload) != header.payload_digest {
        return Err(b"packforge: payload integrity check failed\n");
    }

    let original_mapping = map_writable(original_length)
        .ok_or(b"packforge: cannot allocate output memory\n" as &'static [u8])?;
    let original = unsafe { slice::from_raw_parts_mut(original_mapping, original_length) };
    lz4::decompress(payload, original)
        .map_err(|()| b"packforge: LZ4 decompression failed\n" as &'static [u8])?;
    if hash::hash(original) != header.original_digest {
        return Err(b"packforge: original integrity check failed\n");
    }

    let memfd = create_executable_memfd()?;
    if is_error(syscall2(SYS_FCHMOD, memfd, 0o700)) {
        return Err(b"packforge: cannot set anonymous executable mode\n");
    }
    write_exact(memfd, original)
        .map_err(|()| b"packforge: cannot write anonymous executable\n" as &'static [u8])?;
    if is_error(syscall3(SYS_FCNTL, memfd, F_ADD_SEALS, F_SEAL_ALL)) {
        return Err(b"packforge: cannot seal anonymous executable\n");
    }

    let _ = syscall1(SYS_CLOSE, self_fd);
    let _ = syscall2(SYS_MUNMAP, payload_mapping as usize, payload_length);
    let _ = syscall2(SYS_MUNMAP, original_mapping as usize, original_length);

    let _ = syscall5(
        SYS_EXECVEAT,
        memfd,
        c"".as_ptr() as usize,
        argv as usize,
        envp as usize,
        AT_EMPTY_PATH,
    );
    let mut path = [0u8; 40];
    procfd::format(memfd, &mut path);
    let _ = syscall3(
        SYS_EXECVE,
        path.as_ptr() as usize,
        argv as usize,
        envp as usize,
    );
    Err(b"packforge: anonymous execution failed\n")
}

fn parse_trailer(bytes: &[u8; TRAILER_LEN], file_length: u64) -> Result<Trailer, &'static [u8]> {
    if &bytes[..8] != TRAILER_MAGIC
        || get_u16(bytes, 8) != 1
        || get_u16(bytes, 10) != TRAILER_LEN as u16
    {
        return Err(b"packforge: invalid executable trailer\n");
    }
    let mut hash_input = *bytes;
    let stored_hash = array_32(bytes, TRAILER_HASH_OFFSET);
    hash_input[TRAILER_HASH_OFFSET..].fill(0);
    if hash::hash(&hash_input) != stored_hash {
        return Err(b"packforge: executable trailer integrity failed\n");
    }
    if get_u16(bytes, 12) != 1
        || get_u16(bytes, 14) != 0
        || get_u16(bytes, 80) != 1
        || get_u16(bytes, 82) != 1
        || get_u16(bytes, 84) != 62
        || bytes[86..96].iter().any(|byte| *byte != 0)
    {
        return Err(b"packforge: unsupported executable metadata\n");
    }

    let trailer = Trailer {
        container_offset: get_u64(bytes, 16),
        container_length: get_u64(bytes, 24),
        executable_length: get_u64(bytes, 32),
        loader_length: get_u64(bytes, 40),
        loader_digest: array_32(bytes, 48),
    };
    if trailer.executable_length != file_length
        || trailer.loader_length == 0
        || trailer.loader_length > MAX_STUB_SIZE
        || trailer.container_offset != trailer.loader_length
        || trailer.container_length < CONTAINER_HEADER_LEN as u64
        || trailer.container_length > MAX_CONTAINER_SIZE
        || trailer
            .container_offset
            .checked_add(trailer.container_length)
            != file_length.checked_sub(TRAILER_LEN as u64)
    {
        return Err(b"packforge: invalid executable layout\n");
    }
    Ok(trailer)
}

fn parse_header(
    bytes: &[u8; CONTAINER_HEADER_LEN],
    container_length: u64,
) -> Result<Header, &'static [u8]> {
    if &bytes[..8] != CONTAINER_MAGIC
        || get_u16(bytes, 8) != 1
        || get_u16(bytes, 10) != CONTAINER_HEADER_LEN as u16
    {
        return Err(b"packforge: invalid container header\n");
    }
    let stored_hash = array_32(bytes, HEADER_HASH_OFFSET);
    let mut hash_input = *bytes;
    hash_input[HEADER_HASH_OFFSET..HEADER_HASH_END].fill(0);
    if hash::hash(&hash_input) != stored_hash {
        return Err(b"packforge: container header integrity failed\n");
    }
    if bytes[12] != 1
        || bytes[13] != 1
        || bytes[14] != 1
        || bytes[15] != 2
        || bytes[16] != 1
        || bytes[17] != 0
        || get_u16(bytes, 18) != 62
        || get_u16(bytes, 20) != 2
        || get_u16(bytes, 22) == 0
        || get_u32(bytes, 28) != 0
        || bytes[184..].iter().any(|byte| *byte != 0)
    {
        return Err(b"packforge: unsupported container metadata\n");
    }
    let original_size = get_u64(bytes, 32);
    let payload_size = get_u64(bytes, 40);
    if original_size == 0
        || original_size > MAX_ORIGINAL_SIZE
        || payload_size == 0
        || payload_size > MAX_PAYLOAD_SIZE
        || CONTAINER_HEADER_LEN as u64 + payload_size != container_length
    {
        return Err(b"packforge: invalid container lengths\n");
    }
    if &bytes[56..88] != config_digest(bytes).as_slice() {
        return Err(b"packforge: container configuration integrity failed\n");
    }
    Ok(Header {
        original_size,
        payload_size,
        original_digest: array_32(bytes, 88),
        payload_digest: array_32(bytes, 120),
    })
}

fn config_digest(header: &[u8; CONTAINER_HEADER_LEN]) -> [u8; 32] {
    let mut config = [0u8; 32];
    put_u16(&mut config, 0, 1);
    config[2] = header[12];
    config[3] = header[13];
    config[4] = header[14];
    config[5] = header[15];
    config[6] = header[16];
    put_u16(&mut config, 8, get_u16(header, 18));
    put_u16(&mut config, 10, get_u16(header, 20));
    put_u16(&mut config, 12, get_u16(header, 22));
    put_u32(&mut config, 16, get_u32(header, 28));
    put_u64(&mut config, 20, get_u64(header, 48));
    hash::hash(&config)
}

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

fn create_executable_memfd() -> Result<usize, &'static [u8]> {
    let flags = MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_EXEC;
    let mut result = syscall2(SYS_MEMFD_CREATE, c"packforge".as_ptr() as usize, flags);
    if result == -EINVAL {
        result = syscall2(
            SYS_MEMFD_CREATE,
            c"packforge".as_ptr() as usize,
            MFD_CLOEXEC | MFD_ALLOW_SEALING,
        );
    }
    if is_error(result) {
        return Err(b"packforge: memfd_create unavailable\n");
    }
    Ok(result as usize)
}

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

fn write_exact(fd: usize, mut input: &[u8]) -> Result<(), ()> {
    while !input.is_empty() {
        let result = syscall3(SYS_WRITE, fd, input.as_ptr() as usize, input.len());
        if result <= 0 || is_error(result) {
            return Err(());
        }
        let written = usize::try_from(result).map_err(|_| ())?;
        input = input.get(written..).ok_or(())?;
    }
    Ok(())
}

fn fail(message: &[u8]) -> ! {
    let _ = syscall3(SYS_WRITE, 2, message.as_ptr() as usize, message.len());
    let _ = syscall1(SYS_EXIT_GROUP, 127);
    loop {
        core::hint::spin_loop();
    }
}

const fn is_error(result: isize) -> bool {
    result < 0 && result >= -4095
}

fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([input[offset], input[offset + 1]])
}

fn get_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
        input[offset + 4],
        input[offset + 5],
        input[offset + 6],
        input[offset + 7],
    ])
}

fn array_32(input: &[u8], offset: usize) -> [u8; 32] {
    let mut output = [0u8; 32];
    output.copy_from_slice(&input[offset..offset + 32]);
    output
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memcpy(destination: *mut u8, source: *const u8, count: usize) -> *mut u8 {
    for index in 0..count {
        let byte = unsafe { ptr::read_volatile(source.add(index)) };
        unsafe { ptr::write_volatile(destination.add(index), byte) };
    }
    destination
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
fn syscall5(
    number: usize,
    first: usize,
    second: usize,
    third: usize,
    fourth: usize,
    fifth: usize,
) -> isize {
    syscall6(number, first, second, third, fourth, fifth, 0)
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
