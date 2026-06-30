#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A foreign write through a pre-derived raw pointer while `&mut UnsafeCell<u8>` is
/// under FnEntry protection. SB passes; both HB and TB fail (for different reasons).
///
/// ## Source
/// Ported from `tree_borrows/fail/reserved/cell-protected-write.rs`.
/// That test fails TB. HB also fails, but due to the two-phase activation hook, not
/// the StrongProtector. SB passes.
///
/// ## What this tests
///
/// ```rust
/// let mut n = UnsafeCell::new(0u8);
/// let n_ref: &mut UnsafeCell<u8> = &mut n;
/// let y: *mut u8 = n_ref as *mut UnsafeCell<u8> as *mut u8; // raw alias
/// write_with_protected(n_ref, y);
/// unsafe fn write_with_protected(x: &mut UnsafeCell<u8>, y: *mut u8) {
///     *y = 1;  // foreign write via pre-call raw alias
/// }
/// ```
///
/// ## SB behavior
///
/// SB treats `&mut UnsafeCell<u8>` as SharedReadWrite for interior mutation.
/// FnEntry retag on `x` creates a new SRW tag. The pre-existing alias `y` is also SRW.
/// Writing via `y` (SRW) while `x` holds SRW → allowed. SB verdict: **pass**.
///
/// ## TB behavior
///
/// In TB, `x: &mut UnsafeCell<u8>` after FnEntry retag becomes `ReservedIM` with a
/// StrongProtector. TB rule: foreign write to a Protected ReservedIM location is UB.
/// TB verdict: **fail**.
///
/// ## HB behavior
///
/// HB's two-phase activation hook fires at the CALL SITE of `write_with_protected`,
/// BEFORE the callee's body executes. The hook finds `n_ref` (a `&mut` argument) in
/// the allocation, sees `perms = ReservedIM`, and immediately activates it:
///   `perms = Write`, `shared_borrower = None`  (cleared!)
///
/// By the time `*y = 1` executes inside the function:
///   - `perms = Write` (not ReservedIM), `shared_borrower = None`
///   - T_y was derived via `n_ref as *mut UnsafeCell<u8>`, which goes through the Ref
///     path (not RawPtr), placing T_y in `reborrow_chain` not `exposed_stack`
///   - T_y is not current_borrower, not prev_borrower → ERROR
///
/// The ReservedIM::Write tolerance path (`shared_borrower == Some(T_y)`) is never reached
/// because the hook already cleared `shared_borrower` and activated to Write.
/// HB verdict: **fail** (two-phase activation hook fires too eagerly at the call site).
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **pass**| SRW-based UnsafeCell handling allows foreign writes |
/// | TB    | **fail**| StrongProtector on ReservedIM blocks foreign writes |
/// | HB    | **fail**| Two-phase activation hook clears shared_borrower before callee runs |
unsafe fn write_with_protected(x: &mut UnsafeCell<u8>, y: *mut u8) {
    *y = 1; // foreign write via pre-derived raw alias
    let _ = x;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let mut n = UnsafeCell::new(0u8);
        let n_ref: &mut UnsafeCell<u8> = &mut n;
        let y: *mut u8 = n_ref as *mut UnsafeCell<u8> as *mut u8;
        write_with_protected(n_ref, y);
    }
    0
}
