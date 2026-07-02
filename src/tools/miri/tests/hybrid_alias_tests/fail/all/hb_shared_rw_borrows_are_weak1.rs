#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::Cell;
use core::mem;

// Port of fail/stacked_borrows/shared_rw_borrows_are_weak1.rs
// SB: fail — a SharedReadWrite borrow (via interior mutability) placed below an already
// granted Unique on the stack: writing through the SharedReadWrite invalidates the Unique.
// HB: fail — confirmed. Writing through `shr_rw` (`shr_rw.set(1)`) displaces `y`'s tag as
// current_borrower; `y.get_mut()`'s subsequent reborrow no longer matches.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let x = &mut Cell::new(0);
        let y: &mut Cell<i32> = mem::transmute(&mut *x);
        let shr_rw = &*x;
        shr_rw.set(1);
        y.get_mut();
    }
    0
}
