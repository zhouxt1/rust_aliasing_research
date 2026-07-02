#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// FIXED (2026-07-02): this used to pass in HB (unsound — SB rejects it) and now fails, at the
/// return-site retag itself (retagging a dead entry to build `Some(ret)` for return requires a
/// Read-kind validation, which the dead check now denies), one statement earlier than the
/// caller's `*r = 99`. See COVERAGE.md's "pass/sb_tb_fail case study".
///
/// Returning an `Option<&mut T>` whose inner ref was shadowed by a parent READ is UB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/return_invalid_mut_option.rs` — that test **fails** SB
/// and now **fails** HB too.
///
/// ## What this tests
///
/// Same mechanism as `sb_return_reserved_ref_after_parent_read.rs`, but the `&mut` is
/// wrapped in an `Option`. The key access is `*xraw` (a READ via base_pointer T_xraw).
///
/// In SB this read pops the child Unique `ret` from the stack. In HB (post-fix) the same read
/// marks `ret`'s (T_ret) `RawPointerStack` entry `dead`; wrapping it in `Some(ret)` for return
/// then fails the return-site retag.
///
/// ## HB vs SB
///
/// SB: the `*xraw` read pops T_ret's Unique item → wrapping/returning `Some(ret)` retags T_ret
/// at the return site → T_ret not in stack → UB.
///
/// HB: the `*xraw` read marks T_ret's entry dead → wrapping/returning `Some(ret)` retags T_ret
/// at the return site → dead check denies it → UB.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops ret's Unique item; return-site retag fails |
/// | HB    | **fail**| (post-fix) `*xraw` marks ret's entry dead; return-site retag fails |
fn foo(x: &mut (i32, i32)) -> Option<&mut i32> {
    let xraw = x as *mut (i32, i32);
    let ret = unsafe { &mut (*xraw).1 }; // RawPtr reborrow: T_ret; exposed_stack = [{T_xraw, T_ret}]
    let ret = Some(ret);
    let _val = unsafe { *xraw }; // READ via T_xraw (base_pointer) — kills T_ret's entry (post-fix)
    ret //~ ERROR: killed by an earlier read through its base pointer
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    match foo(&mut (1, 2)) {
        Some(r) => {
            *r = 99; // never reached (post-fix)
        }
        None => {}
    }
    0
}
