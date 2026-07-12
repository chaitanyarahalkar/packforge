//! Bounded four-way codec-4 decoding with raw Linux clone workers.

use core::arch::{asm, global_asm};
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::{bcj, hash, lzma_asm};
use crate::v2_format::{CODEC4_CHUNK_COUNT, Codec4Chunk};

const PAGE_SIZE: usize = 4096;
const STACK_PAGES: usize = 8;
const STACK_STRIDE: usize = (STACK_PAGES + 1) * PAGE_SIZE;
const STACK_MAPPING_LENGTH: usize = 3 * STACK_STRIDE;
const PROT_NONE: usize = 0;
const PROT_READ_WRITE: usize = 3;
const MAP_PRIVATE_ANONYMOUS: usize = 0x22;
const FUTEX_WAIT: usize = 0;
const FUTEX_WAKE: usize = 1;
const SYS_MMAP: usize = 9;
const SYS_MPROTECT: usize = 10;
const SYS_MUNMAP: usize = 11;
const SYS_FUTEX: usize = 202;
const WORKER_COUNT: usize = 3;
const LANE_COUNT: usize = WORKER_COUNT + 1;
const STAGE_PAYLOAD_HASH: i32 = 1;
const STAGE_DECODE: i32 = 2;
const STAGE_ORIGINAL_HASH: i32 = 3;
const STAGE_EXIT: i32 = 4;

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
    xor %rbp, %rbp
    call packforge_worker_entry
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
    hash_input: *const u8,
    hash_input_length: usize,
    hash_cvs: *mut [u32; 8],
    hash_start: usize,
    hash_end: usize,
    stage: AtomicI32,
    status: AtomicI32,
    exit_tid: AtomicI32,
}

pub fn decompress_authenticated(
    payload: &mut [u8],
    output: &mut [u8],
    properties: [u8; 5],
    chunks: [Codec4Chunk; CODEC4_CHUNK_COUNT],
    payload_digest: [u8; 32],
    original_digest: [u8; 32],
) -> Result<(), DecodeError> {
    let stack_mapping = map_stacks()?;
    let payload_cv_count = chunk_cv_count(payload.len());
    let original_cv_count = chunk_cv_count(output.len());
    let payload_workspace_length = workspace_length(payload_cv_count)?;
    let original_workspace_length = workspace_length(original_cv_count)?;
    if payload_workspace_length > output.len() || original_workspace_length > payload.len() {
        let _ = syscall2(SYS_MUNMAP, stack_mapping as usize, STACK_MAPPING_LENGTH);
        return Err(DecodeError);
    }
    let payload_cvs = output.as_mut_ptr().cast::<[u32; 8]>();
    let mut workers: [Worker; 3] = core::array::from_fn(|index| {
        let chunk = chunks[index + 1];
        let (hash_start, hash_end) = lane_range(payload_cv_count, index + 1);
        Worker {
            input: unsafe { payload.as_ptr().add(chunk.compressed_offset) },
            input_length: chunk.compressed_length,
            output: unsafe { output.as_mut_ptr().add(chunk.decoded_offset) },
            output_length: chunk.decoded_length,
            properties,
            trailing_bytes: chunk.trailing_bytes,
            hash_input: payload.as_ptr(),
            hash_input_length: payload.len(),
            hash_cvs: payload_cvs,
            hash_start,
            hash_end,
            stage: AtomicI32::new(STAGE_PAYLOAD_HASH),
            status: AtomicI32::new(0),
            exit_tid: AtomicI32::new(0),
        }
    });
    let mut spawn_ok = true;
    for (index, worker) in workers.iter_mut().enumerate() {
        worker.exit_tid.store(-1, Ordering::Release);
        let stack_top = unsafe { stack_mapping.add((index + 1) * STACK_STRIDE) };
        let result = unsafe { spawn(stack_top, worker, &mut worker.exit_tid) };
        if result < 0 {
            spawn_ok = false;
            worker.exit_tid.store(0, Ordering::Release);
            break;
        }
    }

    let payload_hash = if spawn_ok {
        hash_stage_main(payload, payload_cvs, payload_cv_count, 0)
            .and_then(|()| wait_for_stage(&workers, STAGE_PAYLOAD_HASH))
            .and_then(|()| complete_hash(payload, payload_cvs, payload_cv_count))
    } else {
        Err(DecodeError)
    };
    if payload_hash != Ok(payload_digest) {
        stop_workers(&workers);
        unmap_stacks(stack_mapping);
        return Err(DecodeError);
    }

    start_stage(&workers, STAGE_DECODE);
    let main = chunks[0];
    let main_result = lzma_asm::decompress(
        &payload[main.compressed_offset..main.compressed_offset + main.compressed_length],
        &mut output[main.decoded_offset..main.decoded_offset + main.decoded_length],
        properties,
        main.trailing_bytes,
    )
    .map_err(|_| DecodeError)
    .and_then(|()| wait_for_stage(&workers, STAGE_DECODE));
    if main_result.is_err() || bcj::decode(output).is_err() {
        stop_workers(&workers);
        unmap_stacks(stack_mapping);
        return Err(DecodeError);
    }

    let original_cvs = payload.as_mut_ptr().cast::<[u32; 8]>();
    for (index, worker) in workers.iter_mut().enumerate() {
        let (hash_start, hash_end) = lane_range(original_cv_count, index + 1);
        worker.hash_input = output.as_ptr();
        worker.hash_input_length = output.len();
        worker.hash_cvs = original_cvs;
        worker.hash_start = hash_start;
        worker.hash_end = hash_end;
    }
    start_stage(&workers, STAGE_ORIGINAL_HASH);
    let original_hash = hash_stage_main(output, original_cvs, original_cv_count, 0)
        .and_then(|()| wait_for_stage(&workers, STAGE_ORIGINAL_HASH))
        .and_then(|()| complete_hash(output, original_cvs, original_cv_count));
    stop_workers(&workers);
    unmap_stacks(stack_mapping);
    if original_hash != Ok(original_digest) {
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
unsafe extern "C" fn packforge_worker_entry(worker: *mut Worker) {
    let worker = unsafe { &*worker };
    loop {
        let stage = worker.stage.load(Ordering::Acquire);
        if stage == STAGE_EXIT {
            return;
        }
        let result = if stage == STAGE_DECODE {
            let input = unsafe { slice::from_raw_parts(worker.input, worker.input_length) };
            let output = unsafe { slice::from_raw_parts_mut(worker.output, worker.output_length) };
            lzma_asm::decompress(input, output, worker.properties, worker.trailing_bytes)
                .map_err(|_| DecodeError)
        } else if matches!(stage, STAGE_PAYLOAD_HASH | STAGE_ORIGINAL_HASH) {
            worker_hash_range(worker)
        } else {
            Err(DecodeError)
        };
        worker
            .status
            .store(if result.is_ok() { stage } else { -1 }, Ordering::Release);
        wake(&worker.status);
        wait_for_change(&worker.stage, stage);
    }
}

fn worker_hash_range(worker: &Worker) -> Result<(), DecodeError> {
    let input = unsafe { slice::from_raw_parts(worker.hash_input, worker.hash_input_length) };
    hash_range(
        input,
        worker.hash_cvs,
        worker.hash_start,
        worker.hash_end,
    );
    Ok(())
}

fn hash_stage_main(
    input: &[u8],
    cvs: *mut [u32; 8],
    cv_count: usize,
    lane: usize,
) -> Result<(), DecodeError> {
    let (start, end) = lane_range(cv_count, lane);
    hash_range(input, cvs, start, end);
    Ok(())
}

fn hash_range(input: &[u8], cvs: *mut [u32; 8], start: usize, end: usize) {
    for index in start..end {
        let cv = hash::chunk_chaining_value(input, index);
        unsafe { cvs.add(index).write(cv) };
    }
}

fn complete_hash(
    input: &[u8],
    cvs: *mut [u32; 8],
    cv_count: usize,
) -> Result<[u8; 32], DecodeError> {
    let cvs = unsafe { slice::from_raw_parts(cvs, cv_count) };
    hash::hash_with_chunk_cvs(input, cvs).ok_or(DecodeError)
}

fn chunk_cv_count(length: usize) -> usize {
    length.div_ceil(hash::CHUNK_LEN).max(1) - 1
}

fn workspace_length(cv_count: usize) -> Result<usize, DecodeError> {
    cv_count
        .checked_mul(core::mem::size_of::<[u32; 8]>())
        .ok_or(DecodeError)
}

fn lane_range(count: usize, lane: usize) -> (usize, usize) {
    (count * lane / LANE_COUNT, count * (lane + 1) / LANE_COUNT)
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

fn start_stage(workers: &[Worker; WORKER_COUNT], stage: i32) {
    for worker in workers {
        if worker.exit_tid.load(Ordering::Acquire) != 0 {
            worker.status.store(0, Ordering::Release);
            worker.stage.store(stage, Ordering::Release);
            wake(&worker.stage);
        }
    }
}

fn wait_for_stage(workers: &[Worker; WORKER_COUNT], stage: i32) -> Result<(), DecodeError> {
    for worker in workers {
        if worker.exit_tid.load(Ordering::Acquire) == 0 {
            return Err(DecodeError);
        }
        loop {
            let status = worker.status.load(Ordering::Acquire);
            if status == stage {
                break;
            }
            if status < 0 {
                return Err(DecodeError);
            }
            wait(&worker.status, status);
        }
    }
    Ok(())
}

fn stop_workers(workers: &[Worker; WORKER_COUNT]) {
    for worker in workers {
        if worker.exit_tid.load(Ordering::Acquire) != 0 {
            worker.stage.store(STAGE_EXIT, Ordering::Release);
            wake(&worker.stage);
        }
    }
    for worker in workers {
        wait_for_exit(&worker.exit_tid);
    }
}

fn unmap_stacks(stack_mapping: *mut u8) {
    let _ = syscall2(SYS_MUNMAP, stack_mapping as usize, STACK_MAPPING_LENGTH);
}

fn wait_for_change(value: &AtomicI32, expected: i32) {
    while value.load(Ordering::Acquire) == expected {
        wait(value, expected);
    }
}

fn wait(value: &AtomicI32, expected: i32) {
    let _ = syscall6(
        SYS_FUTEX,
        value.as_ptr() as usize,
        FUTEX_WAIT,
        expected as usize,
        0,
        0,
        0,
    );
}

fn wake(value: &AtomicI32) {
    let _ = syscall6(
        SYS_FUTEX,
        value.as_ptr() as usize,
        FUTEX_WAKE,
        1,
        0,
        0,
        0,
    );
}

fn wait_for_exit(exit_tid: &AtomicI32) {
    loop {
        let expected = exit_tid.load(Ordering::Acquire);
        if expected == 0 {
            return;
        }
        wait(exit_tid, expected);
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
