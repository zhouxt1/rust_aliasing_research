#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Creating a shared reference does not "leak" the allocation back to a raw pointer in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read6.rs` — that test **fails** SB but is
/// expected to **pass** HB.
///
/// ## What this tests
///
/// ```
/// let x = &mut 0;         // T_x = current_borrower
/// let raw = x as *mut _;  // raw carries T_x
/// let x = &mut *x;        // Ref reborrow: T_x2 = current, T_x = prev_borrower
/// let _y = &*x;           // shared reborrow → Read state
/// let _val = *raw;        // READ via T_x (prev_borrower)
/// ```
///
/// The question is whether the shared ref `_y` somehow "locks out" the raw pointer.
/// In HB, `*raw` accesses via T_x (prev_borrower). Reads via the previous borrower are
/// allowed in HB (HB's access check permits prev_borrower reads, as demonstrated by the
/// `pass/hybrid/raw_ptr_from_mut_parent_restore.rs` parent-read pattern). The allocation
/// is currently in `Read` state from `_y`'s perspective, but T_x (prev_borrower) is the
/// Ref-reborrow parent, not a foreign tag.
///
/// ## HB vs SB
///
/// SB: after `let x = &mut *x`, T_x2 (Unique) is pushed above T_x (also Unique). Then
/// `let _y = &*x` pushes a SharedReadOnly above T_x2. Using `*raw` (T_x, Unique, below
/// T_x2 and _y) would pop both T_x2 and _y from the SB stack and succeed — but SB
/// semantics say "creating a shared ref should not re-expose the raw", so it fails.
///
/// HB: does not have a borrow stack. T_x as prev_borrower passes `check_unique_borrower_tag`.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let x = &mut 0i32;
        let raw = x as *mut i32; // raw carries T_x = current_borrower
        let x = &mut *x;        // Ref reborrow: T_x2 = current, T_x = prev_borrower
        let _y = &*x;           // shared reborrow (Read state)
        let _val = *raw;        // READ via T_x (prev_borrower) — HB: allowed
    }
    0
}
