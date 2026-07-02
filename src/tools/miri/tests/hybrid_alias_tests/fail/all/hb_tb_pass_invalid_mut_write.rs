#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// FIXED (2026-07-02): this used to pass in HB (an unsound divergence — both SB and TB reject
/// it) and now correctly fails. See COVERAGE.md's "pass/sb_tb_fail case study" for the full
/// investigation. Fix: `RawPointerStack` entries now carry a `dead` flag; a **read** through an
/// entry's `base_pointer` kills that entry (and everything above it) for all further access,
/// read or write. Only a **write** through the base pointer can reclaim/revive the slot.
///
/// An activated child `&mut` can still be written via FnEntry reborrow after a parent raw READ
/// in HB, even though both SB and TB forbid it.
///
/// ## Source
/// Ported from `tree_borrows/fail/pass_invalid_mut.rs` — that test **fails** TB (and SB)
/// but **passes** HB.
///
/// The SB version (`stacked_borrows/fail/pass_invalid_mut.rs`) also fails, but only requires
/// a read in `foo` rather than a write. This test uses the TB version which requires a write
/// to demonstrate that HB's "parent read does not freeze" property holds even for write access.
///
/// ## What this tests
///
/// Setup:
///   1. `xref = &mut *xraw` — child reborrow from raw ptr.
///   2. `*xref = 18` — activate xref (TB: Reserved → Active).
///   3. `*xraw` (READ) — parent raw read.
///      - SB: pops xref's Unique item.
///      - TB: foreign READ on Active node → Active → Frozen; xref can no longer write.
///      - HB: base-pointer READ does NOT truncate exposed_stack; xref remains valid.
///   4. `foo(xref)` writes `*nope = 31`.
///      - SB: retag fails (xref's tag popped).
///      - TB: write through Frozen node → ERROR.
///      - HB: T_xref still top of exposed_stack → FnEntry retag + write succeed.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops xref; FnEntry retag fails |
/// | TB    | **fail**| parent READ freezes Active xref; write through Frozen node is UB |
/// | HB    | **fail**| (post-fix) `*xraw` marks xref's `RawPointerStack` entry dead; the later write is denied |
#[inline(never)]
fn foo(nope: &mut i32) {
    *nope = 31; // write via FnEntry-retagged reference
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = &mut 42i32;
    let xraw = x as *mut i32; // T_xraw
    let xref = unsafe { &mut *xraw }; // T_xref; exposed_stack = [..., (T_xraw, T_xref)]

    *xref = 18; // activate xref (TB: Reserved → Active; HB: no state change needed)

    let _val = unsafe { *xraw }; // parent raw READ
                                  // SB: kills xref  TB: freezes xref  HB: no effect on xref

    foo(xref); //~ ERROR: killed by an earlier read through its base pointer

    0
}
