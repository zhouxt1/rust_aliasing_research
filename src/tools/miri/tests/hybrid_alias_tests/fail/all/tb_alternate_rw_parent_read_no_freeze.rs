#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// FIXED (2026-07-02), but with a TIMING MISMATCH vs TB — see COVERAGE.md's "pass/sb_tb_fail
/// case study" for the full investigation. This used to pass in HB (an unsound divergence, since
/// TB rejects it) and now fails too, so the two models agree on the *verdict*. However HB's
/// current binary `dead` flag kills a `RawPointerStack` entry on *any* parent read, without
/// distinguishing TB's Reserved (never self-written) vs Active (already self-written) states.
/// As a result HB fails one round earlier than TB — see "Model verdicts" below. Implementing the
/// deferred `activated: bool` refinement (only kill on parent read once the entry has been
/// self-written) would make HB fail at the exact same statement as TB.
///
/// ## Source
/// Ported from `tree_borrows/fail/alternate-read-write.rs` — that test **fails** Tree
/// Borrows but **used to pass** HB.
///
/// ## What this tests
///
/// Setup:
///   x = &mut 0u8
///   y = &mut *(x as *mut u8)   // RawPtr reborrow: T_y; exposed_stack = [{T_x, T_y}]
///
/// Sequence:
///   *x        — READ via T_x (base_pointer)
///   *y += 1   — WRITE via T_y (current_borrower)
///   *x        — READ via T_x again
///   *y += 1   — WRITE via T_y again
///
/// ## Why TB fails this (and exactly when)
///
/// In Tree Borrows, a parent READ only freezes an *Active* (already-written) child; a
/// never-yet-activated `Reserved` child tolerates foreign reads freely. The sequence:
///   1. `let _val = *x` — Foreign Read for y: y is still Reserved (never written) → tolerated
///   2. `*y += 1` — activates y (Reserved → Active); OK, no freeze has happened yet
///   3. `let _val = *x` — Foreign Read for y: y is now Active → transitions to Frozen
///   4. `*y += 1` — Write through Frozen y → UB in TB
///
/// ## Why HB fails this (post-fix) — and why it's one step earlier than TB
///
/// HB's `RawPointerStack.dead` flag has no Reserved-vs-Active distinction: *any* read through
/// the base pointer kills the child entry immediately, even if the child was never activated.
///   1. `let _val = *x` — READ via T_x (base_pointer) → HB (post-fix): kills T_y immediately
///   2. `*y += 1` — write via a now-dead T_y → **ERROR here**, three statements before TB fails
///
/// ## Model verdicts
///
/// | Model | Verdict | Fails at |
/// |-------|---------|----------|
/// | TB    | **fail**| statement 4 (`*y += 1` after the *second* parent read) |
/// | HB    | **fail**| (post-fix) statement 2 (`*y += 1` after the *first* parent read) |
///
/// Both reject the program, but HB is currently *more* conservative than TB because it lacks
/// the Reserved/Active distinction. This is not unsound (rejecting more is safe for a checker
/// meant to catch bugs, though it would reject some technically-valid two-phase-borrow-like
/// patterns) — it's a precision gap, tracked as the `activated` field proposal.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = &mut 0u8;
    let y = unsafe { &mut *(x as *mut u8) }; // T_y; exposed_stack = [{T_x, T_y}]

    let _val = *x;  // READ via T_x (base_pointer) — HB (post-fix): kills T_y immediately
    *y += 1;        //~ ERROR: killed by an earlier read through its base pointer

    let _val = *x;  // never reached (post-fix)
    *y += 1;        // never reached (post-fix); this is where TB itself would fail

    let _val = *x;

    0
}
