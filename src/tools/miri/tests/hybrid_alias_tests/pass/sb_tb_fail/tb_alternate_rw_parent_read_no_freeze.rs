#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Alternating parent reads and child writes is valid in HB: parent reads do not freeze.
///
/// ## Source
/// Ported from `tree_borrows/fail/alternate-read-write.rs` — that test **fails** Tree
/// Borrows but **passes** HB.
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
/// In HB, reads via the base_pointer do NOT kill or freeze the child entry. T_y remains
/// `current_borrower` throughout. All four accesses are valid.
///
/// ## Why TB fails this
///
/// In Tree Borrows, a parent READ causes child `Reserved` nodes to transition to `Frozen`
/// (they can no longer be written through). The sequence:
///   1. `let _val = *x` — Foreign Read for y: y transitions Reserved → Frozen
///   2. `*y += 1` — OK (activation from Reserved, not yet Frozen at first read)
///   3. `let _val = *x` — Foreign Read for y: y is Unique (active), transitions to Frozen
///   4. `*y += 1` — Write through Frozen y → UB in TB
///
/// ## Key HB design choice
///
/// HB does not implement TB's "foreign read freezes Reserved/Active" rule. Parent reads
/// are allowed to coexist with child writes as long as no WRITE via the base_pointer
/// has occurred (which would reclaim the borrow). This is a deliberate simplification.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = &mut 0u8;
    let y = unsafe { &mut *(x as *mut u8) }; // T_y; exposed_stack = [{T_x, T_y}]

    let _val = *x;  // READ via T_x (base_pointer) — HB: does not freeze T_y
    *y += 1;        // WRITE via T_y (current_borrower) — succeeds

    let _val = *x;  // READ via T_x again — HB: still does not freeze T_y
    *y += 1;        // WRITE via T_y — still succeeds

    let _val = *x;

    0
}
