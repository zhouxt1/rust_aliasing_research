#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Returning a `(&mut T,)` tuple whose inner ref was only shadowed by a parent READ is valid.
///
/// ## Source
/// Ported from `stacked_borrows/fail/return_invalid_mut_tuple.rs` — that test **fails** SB
/// but **passes** HB.
///
/// ## What this tests
///
/// Same mechanism as `return_invalid_mut_option.rs` but wrapping the `&mut` in a tuple.
///
/// In SB, `*xraw` (READ via base_pointer) pops the child Unique from the stack. In HB,
/// the base_pointer read does NOT kill the child: T_ret remains current_borrower when the
/// tuple is returned.
///
/// ## HB vs SB
///
/// SB: fails at the return site retag (T_ret not in stack after the parent read).
/// HB: succeeds — T_ret is alive; the caller can index into the tuple and use the ref.
fn foo(x: &mut (i32, i32)) -> (&mut i32,) {
    let xraw = x as *mut (i32, i32);
    let ret = (unsafe { &mut (*xraw).1 },); // RawPtr reborrow: T_ret in tuple
    let _val = unsafe { *xraw }; // READ via T_xraw (base_pointer) — does NOT kill T_ret in HB
    ret
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut pair = (1i32, 2i32);
    let r = foo(&mut pair);
    *r.0 = 99;
    0
}
