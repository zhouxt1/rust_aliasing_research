#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::mem;

// Port of fail/stacked_borrows/illegal_read3.rs
// SB: fail — a &mut smuggled through a union (avoiding retagging) is still invalidated once
// the callee reads through the raw union value.
// HB: pass — confirmed. HB does not do union-avoiding-retag tracking the way SB does; the
// callee's read through the smuggled union value does not invalidate xref1/xref2's tag.

union HiddenRef {
    r: &'static i32,
}

#[inline(never)]
fn callee(xref1: HiddenRef) {
    let _val = unsafe { *xref1.r };
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: i32 = 15;
    let xref1 = &mut x;
    let xref1_sneaky: HiddenRef = unsafe { mem::transmute_copy(&xref1) };
    let xref2 = &mut *xref1;
    callee(xref1_sneaky);
    let _val = *xref2;
    0
}
