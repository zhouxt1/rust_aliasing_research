#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// REGRESSED (2026-07-02): moved here from pass/sb_fail/ after the `RawPointerStack.dead` fix
/// (see COVERAGE.md's "pass/sb_tb_fail case study"; near-duplicate of
/// `hb_return_invalid_mut_raw_read.rs`). `ret` is never activated before `*xraw`, so TB keeps
/// it Reserved and tolerates the read. HB's binary `dead` flag kills `ret`'s entry
/// unconditionally, so the return-site retag now fails. Needs the deferred `activated` field.
///
/// Returning a `&mut` that was only shadowed by a parent READ is valid in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/return_invalid_mut.rs` — that test **fails** SB
/// but **passes** HB.
///
/// ## What this tests
///
/// Inside `foo`:
///   xraw = x as *mut (i32, i32)                  // xraw carries tag T_x (base_pointer)
///   ret  = &mut (*xraw).1  (RawPtr reborrow)     // T_ret = current_borrower of .1 field
///                                                 // exposed_stack = [{T_x, T_ret}]
///   _val = *xraw           (READ via T_x)         // base_pointer read — does NOT kill T_ret in HB
///   return ret             (reborrow for caller)  // T_ret still current_borrower → return OK
///
/// The key step is `*xraw` (parent read). In HB, reading via a base_pointer does not
/// kill the child entry. T_ret remains current_borrower; the return reborrow succeeds.
///
/// ## Why SB fails this
///
/// In SB, `*xraw` (Unique tag) after `ret` (another Unique pushed above xraw's Unique)
/// pops `ret`'s Unique item off the stack. Even a READ pops Unique items in SB. The
/// subsequent retag of `ret` at the return site finds its tag gone → UB.
///
/// ## Key HB design choice (pre-fix; now outdated — see REGRESSED note above)
///
/// Parent reads used to not kill children in HB. Post-fix, they do (unconditionally), which
/// is what causes this regression.
fn foo(x: &mut (i32, i32)) -> &mut i32 {
    let xraw = x as *mut (i32, i32); // carries T_x
    let ret = unsafe { &mut (*xraw).1 }; // RawPtr reborrow: T_ret, stack [{T_x, T_ret}]
    let _val = unsafe { *xraw }; // READ via T_x (base_pointer) — does NOT kill T_ret
    ret // T_ret still current_borrower → return reborrow succeeds
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut pair = (1i32, 2i32);
    let r = foo(&mut pair);
    *r = 99; // use the returned &mut
    0
}
