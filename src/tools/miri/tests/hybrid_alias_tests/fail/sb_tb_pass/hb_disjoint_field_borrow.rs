#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// HB per-allocation coarseness: disjoint field borrows in the same allocation.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | pass    | per-byte stacks — s.0 and s.1 have independent entries |
/// | TB    | pass    | per-byte tree  — s.0 and s.1 have independent nodes    |
/// | HB    | **fail**| per-allocation — both fields share one `BorrowerState`  |
///
/// ## What this tests
///
/// ```rust
/// let mut s = (0i32, 0i32);
/// let ra = &mut s.0;   // T_ra = current_borrower of alloc_s
/// let rb = &mut s.1;   // T_rb = current_borrower of alloc_s; T_ra → prev_borrower
/// *rb = 2;             // WRITE via current (T_rb) → clears prev_borrower (T_ra)
/// *ra = 1;             // HB: T_ra no longer in current or prev → denied
/// ```
///
/// This is an **intentional known limitation** of HB's per-allocation design, not a bug.
/// SB and TB track each byte independently, so two `&mut` borrows of different fields
/// never conflict. HB collapses the whole allocation into a single `BorrowerState`, so
/// the second reborrow (`rb`) displaces the first (`ra`) from `current_borrower`, and a
/// write through `rb` clears `ra` from `prev_borrower`. Accessing `ra` afterwards fails.
///
/// ## Why this is acceptable in HB
///
/// HB is a prototype model. Per-allocation granularity is a deliberate trade-off:
/// cheaper state (one record per allocation instead of one per byte) at the cost of
/// false positives for disjoint-field access. A future per-field or per-byte HB variant
/// would handle this correctly.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut s = (0i32, 0i32);

    let ra = &mut s.0; // T_ra = current_borrower; prev = prior tag
    let rb = &mut s.1; // T_rb = current_borrower; T_ra → prev_borrower

    *rb = 2; // WRITE via current (T_rb) → clears prev_borrower (T_ra)
    *ra = 1; // HB: T_ra is neither current nor prev → UB
             // SB: s.0 has its own independent borrow stack → OK
             // TB: s.0 has its own independent tree node  → OK

    0
}
