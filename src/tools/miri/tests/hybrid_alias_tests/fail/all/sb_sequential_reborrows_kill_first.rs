#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Two sequential `&mut` reborrows of the same place: the second kills the first.
///
/// ## Source
/// Ported from `stacked_borrows/fail/raw_tracking.rs`.
///
/// ## What this tests
///
/// When `raw1 = &mut l as *mut i32` is created, HB sets `current_borrower = T1` for
/// `l`'s allocation. `raw1` carries provenance tag T1.
///
/// When `raw2 = &mut l as *mut i32` is created immediately after, this is a new Ref
/// reborrow of `l`: HB sets `current_borrower = T2`, and T1 is displaced (it becomes
/// `prev_borrower` or is simply no longer the active borrower).
///
/// Any subsequent write through `raw1` (which carries T1) is UB: T1 is no longer
/// the current_borrower and cannot be found in the exposed stack.
///
/// ## HB vs SB
///
/// SB: tracks unique tags per pointer; `raw2`'s reborrow pops everything above
/// the root tag on the borrow stack, invalidating `raw1`'s Unique item.
/// HB: same outcome via the single-current-borrower model — T1 is displaced.
///
/// ## Difference from fail/sibling_mut_from_same_raw.rs
///
/// Here both raw pointers are derived directly from `l` (independent `&mut l` reborrows).
/// In `sibling_mut_from_same_raw.rs`, both are derived from the same intermediate raw via
/// `&mut *raw`. The failure mechanism in HB is the same (second reborrow kills first), but
/// the reborrow chain differs.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut l = 13i32;

    let raw1 = &mut l as *mut i32; // T1 = current_borrower, raw1 carries T1
    let raw2 = &mut l as *mut i32; // T2 = new current_borrower, T1 displaced

    unsafe { *raw1 = 13 }; //~ ERROR: raw1 carries T1, which is no longer current_borrower
    unsafe { *raw2 = 13 };

    0
}
