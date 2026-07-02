#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// STILL UNFIXED (2026-07-02) after the `RawPointerStack.dead` fix — see COVERAGE.md's
/// "pass/sb_tb_fail case study" for why. Unlike the 5 tests that *were* fixed by that change,
/// this one uses a bare `mref as *mut u8` cast rather than a `&mut *raw_ptr` reborrow. Per the
/// existing code comment in `mod.rs` ("Like SB, raw pointers are only retagged for
/// `RetagKind::Raw`"), casting FROM an existing `&mut` does not create any new tracked identity
/// at all — `ptr` just carries `T_mref` directly (`BorrowerState.current_borrower`), and no
/// `RawPointerStack` entry is ever pushed. The `dead`-on-parent-read fix only applies to
/// `RawPointerStack` entries, so it has nothing to act on here: there is no separate
/// "child" tag distinct from `current_borrower` for a parent read to kill. Fixing this test
/// would require a deeper structural change — making bare-cast raw pointers trackable in their
/// own right, distinct from the plain-reference `current_borrower` model — not just the
/// `RawPointerStack`-local fix implemented so far.
///
/// A parent read does NOT kill a raw pointer derived from a child reborrow in HB.
///
/// ## Source
/// Ported from `tree_borrows/fail/parent_read_freezes_raw_mut.rs` — that test **fails**
/// Tree Borrows but **passes** HB.
///
/// ## What this tests
///
/// Setup:
///   root: u8
///   mref = &mut root             // T_mref = current_borrower
///   ptr  = mref as *mut u8       // ptr carries T_mref (no RawPtr reborrow, just cast)
///
/// Sequence:
///   *ptr = 0            — WRITE via T_mref (current_borrower) → OK
///   root == 0           — READ of root (local binding, T_root = original prev_borrower)
///   *ptr = 0            — WRITE via T_mref again
///
/// The READ of `root` (via prev_borrower T_root or direct local access) does not kill
/// T_mref in HB. T_mref remains the current_borrower; the second `*ptr = 0` succeeds.
///
/// ## Why TB fails this
///
/// In Tree Borrows, the `assert_eq!(root, 0)` read is a "parent read" for `mref` and
/// `ptr`. TB's rule: a parent read causes child `Unique` nodes to be `Frozen` (no more
/// writes). The second `*ptr = 0` write through a Frozen node → UB in TB.
///
/// ## Key HB design choice
///
/// HB does not implement TB's parent-read-freezes rule. Only parent WRITES reclaim
/// the borrow and invalidate children. A parent read does not change the child's
/// borrower state.
///
/// ## Contrast
///
/// `fail/raw_ptr_invalid_after_parent_reclaim.rs` shows the WRITE version: once the
/// parent WRITES (`*ref1 += 1`), the child raw pointer is dead.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut root = 6u8;
    let mref = &mut root; // T_mref = current_borrower
    let ptr = mref as *mut u8; // ptr carries T_mref (same tag, just cast)

    unsafe {
        *ptr = 0; // WRITE via T_mref → OK

        // READ of root (parent) — TB: freezes ptr's Unique node
        //                         HB: no effect on T_mref, ptr remains valid
        let _check = root == 0;

        *ptr = 0; // WRITE via T_mref — HB: still current_borrower → OK
    }

    0
}
