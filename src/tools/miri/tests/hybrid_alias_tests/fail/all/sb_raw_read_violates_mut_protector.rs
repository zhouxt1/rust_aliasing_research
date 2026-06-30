#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Accessing an aliased raw pointer while a `&mut` argument protector is active is UB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/invalidate_against_protector1.rs`.
///
/// ## What this tests
///
/// `inner` receives two arguments that alias the same allocation:
/// - `x: *mut i32` — the raw pointer carrying T_xraw
/// - `_y: &mut i32` — a new Ref reborrow of `*xraw` with a `StrongProtector`
///
/// At function entry, FnEntry retag fires for `_y`: HB mints a new tag T_y and
/// installs it in `protected_tags` as a `StrongProtector`.
///
/// Any access via `x` (which carries T_xraw, the base_pointer of T_y's entry)
/// during the call violates the protector: `check_raw_pointer_stack` detects that
/// the displaced tag T_y is protected, and denies the access.
///
/// In SB, the equivalent check fires because any use of `x` pops T_y's Unique
/// item off the stack, and the protector triggers on pop.
///
/// ## HB vs SB
///
/// Both models reject this. The error sites differ slightly:
/// - SB: error may be reported at the point `x` pops T_y (during access)
/// - HB: error is at `*x` inside the callee, caught by the protector check in
///   `check_raw_pointer_stack`
fn inner(x: *mut i32, _y: &mut i32) {
    // _y has a StrongProtector from FnEntry retag.
    // x aliases _y: accessing x displaces _y's protected tag.
    let _val = unsafe { *x }; //~ ERROR: access through aliased raw violates &mut protector
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 0i32;
    let xraw = &mut x as *mut i32;
    let xref = unsafe { &mut *xraw }; // xref derived from xraw (RawPtr reborrow)
    inner(xraw, xref); // xraw and xref alias the same allocation
    0
}
