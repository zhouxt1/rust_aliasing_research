#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// REGRESSED (2026-07-02): moved here from pass/sb_fail/ after the `RawPointerStack.dead` fix
/// (see COVERAGE.md's "pass/sb_tb_fail case study"). `xref` is never written to — only read, at
/// the very end — so it is TB-Reserved throughout and TB tolerates the callee's foreign read.
/// HB's binary `dead` flag kills `xref`'s entry unconditionally on the callee's `*xraw`, so the
/// final `*xref` now fails. This is the same missing Reserved-vs-Active distinction (tracked as
/// the deferred `activated` field) seen across this whole batch of regressed tests.
///
/// A callee reading via the parent raw pointer does NOT kill a child `&mut` in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read1.rs` — that test **fails** SB but
/// **passes** HB.
///
/// ## What this tests
///
/// Setup:
///   xraw = &mut x as *mut _       // T_xraw = current_borrower
///   xref = &mut *xraw             // RawPtr reborrow: T_xref; exposed_stack = [{T_xraw, T_xref}]
///
/// The callee does `*xraw` — a READ via T_xraw, which is the `base_pointer` of xref's
/// exposed-stack entry.
///
/// In HB's model, **reads via a base_pointer do not kill the child current_borrower**.
/// T_xref remains in the exposed stack as `current_borrower` of its entry after the
/// callee returns.
///
/// The final `*xref` read succeeds because T_xref is still the current_borrower.
///
/// ## Why SB fails this
///
/// In Stacked Borrows, any access (read or write) through `xraw` after `xref` (a Unique
/// item) was pushed on top of it pops `xref`'s Unique item off the stack. SB treats
/// reads and writes symmetrically for Unique items. After the callee reads via `xraw`,
/// `xref`'s item is gone, so the final `*xref` read fails.
///
/// ## Key HB design choice (pre-fix; now outdated — see REGRESSED note above)
///
/// HB used to only kill child entries when the parent performs a **write** (which reclaims
/// the borrow), leaving reads via the base_pointer to pass through undisturbed. Post-fix, HB
/// kills on *any* base-pointer read, which is what causes this regression.
fn callee_reads_via_raw(xraw: *mut i32) {
    let _val = unsafe { *xraw }; // READ via base_pointer T_xraw — HB: does not kill T_xref
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 15i32;
    let xraw = &mut x as *mut i32; // T_xraw = current_borrower

    // RawPtr reborrow: T_xref pushed onto exposed stack, current_borrower = T_xref
    let xref = unsafe { &mut *xraw };

    callee_reads_via_raw(xraw); // callee reads *xraw — HB: xref survives

    let _val = *xref; // T_xref still current_borrower → read succeeds

    0
}
