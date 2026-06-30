#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A parent write reclaims the borrow, killing a raw pointer derived from an intermediate reborrow.
///
/// ## Source
/// Ported from `stacked_borrows/fail/disable_mut_does_not_merge_srw.rs` (no imports needed).
///
/// ## What this tests
///
/// ```
/// let base = &mut mem as *mut i32;   // T_base = current_borrower
/// let raw = { &mut *base as *mut _ }; // T_mutref = current, T_base = prev; raw carries T_mutref
/// let _val = *base;                   // READ via T_base (prev_borrower) — does not kill T_mutref
/// *base = 1;                          // WRITE via T_base (prev_borrower) — kills T_mutref, T_base restored
/// let _val = *raw;                    // ERROR: T_mutref is dead (killed by parent write above)
/// ```
///
/// The test demonstrates that a parent WRITE (not just a read) reclaims the borrow and
/// invalidates the child entry. This is consistent with `fail/hybrid/raw_ptr_invalid_after_parent_reclaim.rs`.
///
/// ## HB vs SB
///
/// SB: "disabling" the mutable `mutref` on a parent read transitions it from Unique to
/// Disabled. The parent write then "merges" adjacent SRW groups... the test is specifically
/// about SRW group merging not happening. HB doesn't have SRW groups; the failure mechanism
/// is simpler: the parent WRITE kills the child.
///
/// In HB:
/// 1. `*base` READ (T_base = prev_borrower): allowed, does NOT kill T_mutref.
/// 2. `*base = 1` WRITE (T_base = prev_borrower): reclaims the borrow, T_mutref killed,
///    T_base becomes new current_borrower.
/// 3. `*raw` (T_mutref): T_mutref is dead → UB.
///
/// ## Note
///
/// The SB test name refers to "merging SharedReadWrite groups" — an SB-specific concern
/// that has no analogue in HB. The test is included here because the observable behaviour
/// (raw pointer dead after parent write) is identical across models.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let mut mem = 0i32;
        let base = &mut mem as *mut i32; // T_base = current_borrower

        // Block creates a local mutref, takes its address as raw, then mutref goes out of scope.
        // raw carries T_mutref; T_base = prev_borrower.
        let raw = {
            let mutref = &mut *base; // T_mutref = new current, T_base = prev
            mutref as *mut i32       // raw carries T_mutref
        };
        // mutref scope ends; but T_mutref is still "live" in HB (no Polonius return fact yet)

        let _val = *base; // READ via T_base (prev_borrower) — does NOT kill T_mutref in HB

        *base = 1; // WRITE via T_base (prev_borrower) → kills T_mutref, T_base restored

        let _val = *raw; //~ ERROR: T_mutref was killed by the parent write above
    }
    0
}
