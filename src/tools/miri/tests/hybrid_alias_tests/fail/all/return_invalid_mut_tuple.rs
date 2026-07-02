#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// FIXED (2026-07-02): this used to pass in HB (unsound — SB rejects it) and now fails at the
/// return-site retag itself, before the caller ever sees `r`. See COVERAGE.md's "pass/sb_tb_fail
/// case study".
///
/// Returning a `(&mut T,)` tuple whose inner ref was shadowed by a parent READ is UB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/return_invalid_mut_tuple.rs` — that test **fails** SB
/// and now **fails** HB too.
///
/// ## What this tests
///
/// Same mechanism as `return_invalid_mut_option.rs` but wrapping the `&mut` in a tuple.
///
/// In SB, `*xraw` (READ via base_pointer) pops the child Unique from the stack. In HB
/// (post-fix), the same read marks T_ret's `RawPointerStack` entry dead; the return-site
/// retag of the tuple's inner reference is then denied.
///
/// ## HB vs SB
///
/// SB: fails at the return site retag (T_ret not in stack after the parent read).
/// HB: fails at the return site retag too (T_ret's entry is dead after the parent read).
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops ret's Unique item; return-site retag fails |
/// | HB    | **fail**| (post-fix) `*xraw` marks ret's entry dead; return-site retag fails |
fn foo(x: &mut (i32, i32)) -> (&mut i32,) {
    let xraw = x as *mut (i32, i32);
    let ret = (unsafe { &mut (*xraw).1 },); // RawPtr reborrow: T_ret in tuple
    let _val = unsafe { *xraw }; // READ via T_xraw (base_pointer) — kills T_ret's entry (post-fix)
    ret //~ ERROR: killed by an earlier read through its base pointer
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut pair = (1i32, 2i32);
    let r = foo(&mut pair);
    *r.0 = 99; // never reached (post-fix)
    0
}
