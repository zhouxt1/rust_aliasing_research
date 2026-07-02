#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// FIXED (2026-07-02): this used to pass in HB (unsound — both SB and TB reject it) and now
/// fails, at the return-site retag itself (retagging a dead entry for return requires a Read-
/// kind validation, which the dead check now denies) rather than at the caller's `*ret = 3`.
/// See COVERAGE.md's "pass/sb_tb_fail case study".
///
/// A returned `&mut` can be written after a parent raw READ in HB, even though
/// both SB and TB forbid it.
///
/// ## Source
/// Ported from `tree_borrows/fail/return_invalid_mut.rs` — that test **fails** TB (and SB)
/// but **passes** HB.
///
/// ## What this tests
///
/// Setup (inside `borrow_second_field`):
///   1. `xraw = x as *mut _` — raw pointer alias of the tuple.
///   2. `ret = &mut (*xraw).1` — child `&mut i32` reborrow via the raw pointer.
///   3. `*ret = *ret` — activate `ret` (TB: Reserved → Active).
///   4. `*xraw` (READ) — parent raw READ through the base pointer.
///      - SB: pops ret's Unique item → ret dead.
///      - TB: foreign READ on Active node → ret becomes Frozen for writes.
///      - HB: base-pointer READ does NOT truncate exposed_stack.
///   5. `ret` is returned to the caller.
///
/// In the caller:
///   6. `*ret = 3` — write via the returned reference.
///      - SB/TB: ERROR (ret was invalidated).
///      - HB: T_ret still top of exposed_stack → write succeeds.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops ret's Unique item; write after return fails |
/// | TB    | **fail**| parent READ freezes Active ret; write through Frozen node is UB |
/// | HB    | **fail**| (post-fix) fails at the return-site retag, one statement earlier than SB/TB |
#[inline(never)]
fn borrow_second_field(x: &mut (i32, i32)) -> &mut i32 {
    let xraw = x as *mut (i32, i32);
    let ret = unsafe { &mut (*xraw).1 }; // child &mut via raw ptr

    *ret = *ret; // activate (TB: Reserved → Active)

    let _val = unsafe { *xraw }; // parent raw READ: SB kills ret, TB freezes ret, HB kills (post-fix)

    ret //~ ERROR: killed by an earlier read through its base pointer
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut arg = (1i32, 2i32);
    let ret = borrow_second_field(&mut arg);
    *ret = 3; // never reached — execution already failed inside borrow_second_field (post-fix)
    0
}
