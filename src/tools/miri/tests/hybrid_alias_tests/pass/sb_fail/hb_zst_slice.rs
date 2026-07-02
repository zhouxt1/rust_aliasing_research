#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/zst_slice.rs
// SB: fail — retagging a zero-length slice is a special "ZST fast path" that never registers a
// real tag/stack entry for the underlying bytes, so a subsequent offset-and-read through
// `.as_ptr().add(1)` finds "tag does not exist in the borrow stack".
// HB: candidate hb_pass — HB's BorrowerState is per-allocation, not per-byte/per-slice-window,
// so even if the ZST slice retag is a no-op, the original array `a`'s allocation-level tag
// should still validly cover offset 1. Confirm by running.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let a = [1i32, 2, 3];
        let s = &a[0..0];
        let _v = *s.as_ptr().add(1);
        0
    }
}
