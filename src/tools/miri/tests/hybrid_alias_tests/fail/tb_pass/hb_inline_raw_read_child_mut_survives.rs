#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// REGRESSED (2026-07-02): moved here from pass/sb_fail/ after the `RawPointerStack.dead` fix
/// (see COVERAGE.md's "pass/sb_tb_fail case study"). `xref2` is never written to — only read,
/// both before and after the base-pointer read — so it is TB-Reserved throughout, and TB
/// tolerates a foreign read on a Reserved node. HB's binary `dead` flag kills `xref2`'s entry on
/// the `*xraw` read regardless of activation state, so `*xref2` afterward now fails. This is the
/// missing Reserved-vs-Active distinction tracked as the deferred `activated` field.
///
/// Inline base-pointer READ does NOT kill a child `&mut` reborrow in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read4.rs` — that test **fails** SB but
/// **passes** HB.
///
/// ## What this tests
///
/// In SB, any access through a parent/raw-pointer tag — even a read — pops all child Unique
/// items above it on the borrow stack. So `*xraw` (read via xraw) invalidates `xref2`
/// (a Unique item derived from xraw via `&mut *xraw`), and the subsequent `*xref2` fails.
///
/// In HB, `check_raw_pointer_stack` with `kind = AccessKind::Read` explicitly preserves
/// child entries: the `base_pointer` READ case returns `Ok(())` without truncating the
/// exposed_stack. So `xref2` stays valid after `*xraw` read.
///
/// ## Key difference from `sb_raw_read_doesnt_kill_child_mut.rs`
///
/// That test has the base-pointer read happen inside a **callee** (function call). This
/// test performs the read **inline** (directly in the same scope), verifying that the
/// protection applies at every call site, not only across function-call boundaries.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` read pops `xref2`'s Unique stack item |
/// | TB    | pass    | TB also preserves children on parent reads |
/// | HB    | **fail**| (post-fix regression) READ via base_pointer kills xref2 unconditionally |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 2i32;
    let xref1 = &mut x; // T_xref1 = current_borrower
    let xraw = xref1 as *mut i32; // Raw retag: T_xraw derived from T_xref1;
                                   // exposed_stack = [(T_xref1, T_xraw)]
    let xref2 = unsafe { &mut *xraw }; // RawPtr reborrow: T_xref2;
                                        // exposed_stack = [(T_xref1, T_xraw), (T_xraw, T_xref2)]

    let _val = unsafe { *xraw }; // READ via T_xraw (base_pointer of xref2's entry)
                                  // SB: pops xref2's Unique item → xref2 dead
                                  // HB: base_pointer READ does not truncate exposed_stack
    let _check = *xref2; // T_xref2 still at top of exposed_stack → read succeeds

    0
}
