#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Writing via a raw pointer derived from a shared (frozen) reference is UB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_write3.rs` (no imports needed).
///
/// ## What this tests
///
/// ```
/// let target = 42i32;
/// let r = &target;              // shared ref → BorrowerPermission::Read
/// let ptr = r as *const _ as *mut _;  // raw ptr derived from shared ref
/// unsafe { *ptr = 42 };         // WRITE while in Read state → UB
/// ```
///
/// After `&target` creates a shared borrow, the allocation is in `BorrowerPermission::Read`.
/// Casting the shared ref to `*mut i32` via `*const` cast doesn't change the borrow state.
/// Any write while in `Read` state is unconditionally denied by HB.
///
/// ## HB vs SB
///
/// SB: the error message says "only grants SharedReadOnly permission" — the SharedReadOnly
/// item in the stack does not permit writes, and writing via the raw ptr violates this.
///
/// HB: `BorrowerPermission::Read` unconditionally rejects writes via any tag. The check
/// fires at `*ptr = 42`.
///
/// ## Why this differs from fail/hybrid/raw_write_during_shared_borrow.rs
///
/// In `raw_write_during_shared_borrow.rs`, the raw pointer was created BEFORE the shared
/// reborrow (and thus has a different tag from the shared_borrower). Here the raw pointer
/// IS derived from the shared ref — it carries the shared tag T_ref or a tag derived from it.
/// Both patterns end in the same HB error (write in Read state), but the derivation path differs.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let target = 42i32;
    let r = &target;                            // freeze → BorrowerPermission::Read
    let ptr = r as *const i32 as *mut i32;      // raw ptr from shared ref
    unsafe { *ptr = 42; } //~ ERROR: write while allocation is in Read state
    let _val = *r;
    0
}
