//! Bounded four-way codec-4 decoding with raw Linux clone workers.

use core::arch::{asm, global_asm};
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::lzma_asm;
use crate::v2_format::{CODEC4_CHUNK_COUNT, Codec4Chunk};

const PAGE_SIZE: usize = 4096;
const STACK_PAGES: usize = 8;
const STACK_STRIDE: usize = (STACK_PAGES + 1) * PAGE_SIZE;
const STACK_MAPPING_LENGTH: usize = 3 * STACK_STRIDE;
const PROT_NONE: usize = 0;
const PROT_READ_WRITE: usize = 3;
const MAP_PRIVATE_ANONYMOUS: usize = 0x22;
const FUTEX_WAIT: usize = 0;
const SYS_MMAP: usize = 9;
const SYS_MPROTECT: usize = 10;
const SYS_MUNMAP: usize = 11;
const SYS_FUTEX: usize = 202;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

global_asm!(
    r#"
    .hidden packforge_clone_worker
    .hidden packforge_worker_entry
    .global packforge_clone_worker
    .type packforge_clone_worker,@function
packforge_clone_worker:
    sub $8, %rdi
    mov %rsi, (%rdi)
    mov %rdx, %r10
    mov %rdi, %rsi
    mov $0x1250f00, %rdi
    xor %rdx, %rdx
    xor %r8, %r8
    mov $56, %rax
    syscall
    test %rax, %rax
    jnz .Lclone_parent
    pop %rdi
    mov %rdi, %rbx
    xor %rbp, %rbp
    call packforge_worker_entry
    mov %eax, 40(%rbx)
    xor %edi, %edi
    mov $60, %eax
    syscall
    ud2
.Lclone_parent:
    ret
    .size packforge_clone_worker, .-packforge_clone_worker
"#,
    options(att_syntax)
);

#[repr(C)]
struct Worker {
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_length: usize,
    properties: [u8; 5],
    trailing_bytes: u8,
    status: AtomicI32,
    exit_tid: AtomicI32,
}

pub fn decompress(
    payload: &[u8],
    output: &mut [u8],
    properties: [u8; 5],
    chunks: [Codec4Chunk; CODEC4_CHUNK_COUNT],
) -> Result<(), DecodeError> {
    let stack_mapping = map_stacks()?;
    let mut workers: [Worker; 3] = core::array::from_fn(|index| {
        let chunk = chunks[index + 1];
        Worker {
            input: unsafe { payload.as_ptr().add(chunk.compressed_offset) },
            input_length: chunk.compressed_length,
            output: unsafe { output.as_mut_ptr().add(chunk.decoded_offset) },
            output_length: chunk.decoded_length,
            properties,
            trailing_bytes: chunk.trailing_bytes,
            status: AtomicI32::new(0),
            exit_tid: AtomicI32::new(0),
        }
    });
    for (index, worker) in workers.iter_mut().enumerate() {
        worker.exit_tid.store(-1, Ordering::Release);
        let stack_top = unsafe { stack_mapping.add((index + 1) * STACK_STRIDE) };
        let result = unsafe { spawn(stack_top, worker, &mut worker.exit_tid) };
        if result < 0 {
            worker.status.store(-2, Ordering::Release);
            worker.exit_tid.store(0, Ordering::Release);
            break;
        }
    }

    let main = chunks[0];
    let main_result = lzma_asm::decompress(
        &payload[main.compressed_offset..main.compressed_offset + main.compressed_length],
        &mut output[main.decoded_offset..main.decoded_offset + main.decoded_length],
        properties,
        main.trailing_bytes,
    );
    for worker in &workers {
        wait_for_exit(&worker.exit_tid);
    }
    let workers_ok = workers
        .iter()
        .all(|worker| worker.status.load(Ordering::Acquire) == 1);
    let _ = syscall2(SYS_MUNMAP, stack_mapping as usize, STACK_MAPPING_LENGTH);
    if main_result.is_err() || !workers_ok {
        return Err(DecodeError);
    }
    Ok(())
}

unsafe fn spawn(stack_top: *mut u8, worker: *mut Worker, child_tid: *mut AtomicI32) -> isize {
    let result: isize;
    unsafe {
        asm!(
            "call packforge_clone_worker",
            in("rdi") stack_top,
            in("rsi") worker,
            in("rdx") child_tid,
            lateout("rax") result,
            clobber_abi("C"),
        );
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn packforge_worker_entry(worker: *mut Worker) -> i32 {
    let worker = unsafe { &*worker };
    let input = unsafe { slice::from_raw_parts(worker.input, worker.input_length) };
    let output = unsafe { slice::from_raw_parts_mut(worker.output, worker.output_length) };
    if lzma_asm::decompress(input, output, worker.properties, worker.trailing_bytes).is_ok() {
        1
    } else {
        -1
    }
}

fn map_stacks() -> Result<*mut u8, DecodeError> {
    let mapping = syscall6(
        SYS_MMAP,
        0,
        STACK_MAPPING_LENGTH,
        PROT_READ_WRITE,
        MAP_PRIVATE_ANONYMOUS,
        usize::MAX,
        0,
    );
    if is_error(mapping) {
        return Err(DecodeError);
    }
    let mapping = mapping as *mut u8;
    for index in 0..3 {
        let guard = unsafe { mapping.add(index * STACK_STRIDE) };
        if is_error(syscall3(SYS_MPROTECT, guard as usize, PAGE_SIZE, PROT_NONE)) {
            let _ = syscall2(SYS_MUNMAP, mapping as usize, STACK_MAPPING_LENGTH);
            return Err(DecodeError);
        }
    }
    Ok(mapping)
}

fn wait_for_exit(exit_tid: &AtomicI32) {
    loop {
        let expected = exit_tid.load(Ordering::Acquire);
        if expected == 0 {
            return;
        }
        let _ = syscall6(
            SYS_FUTEX,
            exit_tid.as_ptr() as usize,
            FUTEX_WAIT,
            expected as usize,
            0,
            0,
            0,
        );
    }
}

fn syscall2(number: usize, first: usize, second: usize) -> isize {
    syscall6(number, first, second, 0, 0, 0, 0)
}

fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    syscall6(number, first, second, third, 0, 0, 0)
}

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

const fn is_error(value: isize) -> bool {
    value < 0
}
