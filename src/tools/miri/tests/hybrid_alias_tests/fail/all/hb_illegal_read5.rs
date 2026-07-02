#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::RefCell;
use core::{mem, ptr};

// Port of fail/stacked_borrows/illegal_read5.rs
// SB: fail — can have aliasing &RefCell<T> and &mut T, but reading through the RefCell alias
// (via ptr::read) invalidates the outstanding &mut.
// HB: fail — confirmed. Reading through the RefCell alias (`ptr::read(xshr)`) invalidates
// the outstanding `&mut i32` (`xref`); the subsequent read through `xref` is denied.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let rc = RefCell::new(0);
    let mut refmut = rc.borrow_mut();
    let xref: &mut i32 = &mut *refmut;
    let xshr = &rc;
    let _val = *xref;
    mem::forget(unsafe { ptr::read(xshr) });
    let _val = *xref;
    0
}
