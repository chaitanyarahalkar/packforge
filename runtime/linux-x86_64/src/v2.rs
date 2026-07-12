#![no_main]
#![no_std]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

global_asm!(
    r#"
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
fn panic(_info: &PanicInfo<'_>) -> ! {
    fail(b"packforge: v2 runtime panic\n")
}

#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

#[unsafe(no_mangle)]
extern "C" fn runtime_main(_stack: *const usize, _rtld_fini: usize) -> ! {
    let decoder: fn(
        &[u8],
        &[u8; 5],
        &mut [u8],
    ) -> Result<
        packforge_lzma_decoder::DecodeReport,
        packforge_lzma_decoder::DecodeError,
    > = packforge_lzma_decoder::decompress;
    core::hint::black_box(decoder);
    fail(b"packforge: v2 runtime is not integrated\n")
}

#[inline(always)]
fn fail(message: &[u8]) -> ! {
    unsafe {
        let _ = syscall3(1, 2, message.as_ptr() as usize, message.len());
        let _ = syscall3(231, 1, 0, 0);
        core::hint::unreachable_unchecked();
    }
}

#[inline(always)]
unsafe fn syscall3(number: usize, first: usize, second: usize, third: usize) -> isize {
    let result: isize;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number as isize => result,
            in("rdi") first,
            in("rsi") second,
            in("rdx") third,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    result
}
