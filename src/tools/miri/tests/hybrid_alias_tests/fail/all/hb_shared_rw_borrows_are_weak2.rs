#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::RefCell;
use core::mem;

// Port of fail/stacked_borrows/shared_rw_borrows_are_weak2.rs
// SB: fail — a SharedReadWrite borrow placed below an existing SharedReadWrite: writing
// through it invalidates the earlier SharedReadWrite borrow.
// HB: fail — confirmed. Writing through `shr_rw` (`shr_rw.replace(1)`) invalidates `y`'s tag;
// the subsequent read through `y` is denied.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let x = &mut RefCell::new(0);
        let y: &i32 = mem::transmute(&*x.borrow());
        let shr_rw = &*x;
        shr_rw.replace(1);
        let _val = *y;
    }
    0
}
