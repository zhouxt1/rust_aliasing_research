#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Passing a `&mut` whose parent was only READ (not written) is valid in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/pass_invalid_mut.rs` — that test **fails** SB
/// but **passes** HB.
///
/// ## What this tests
///
///   x    = &mut 42                               // T_x = current_borrower
///   xraw = x as *mut _                           // xraw carries T_x
///   xref = &mut *xraw  (RawPtr reborrow)         // T_xref; exposed_stack = [{T_x, T_xref}]
///   _val = *xraw       (READ via T_x)             // base_pointer read — does NOT kill T_xref
///   foo(xref)          (FnEntry retag for &mut)   // T_xref still current_borrower → retag OK
///
/// The READ `*xraw` does not invalidate T_xref in HB (parent reads don't kill children).
/// At the call to `foo(xref)`, T_xref is still the current_borrower; FnEntry retag
/// mints T_foo_arg, call executes, T_xref is restored on return.
///
/// ## Why SB fails this
///
/// SB: `*xraw` (a Unique access) after `xref` (another Unique pushed on top) pops
/// `xref`'s item. The FnEntry retag at the call site finds `xref`'s tag absent → UB.
///
/// ## Companion tests
///
/// - `sb_raw_read_doesnt_kill_child_mut.rs` — same property, direct read not via function
/// - `sb_return_reserved_ref_after_parent_read.rs` — same property, returned ref
fn consume(_: &mut i32) {}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = &mut 42i32;
    let xraw = x as *mut i32; // carries T_x
    let xref = unsafe { &mut *xraw }; // T_xref; stack [{T_x, T_xref}]
    let _val = unsafe { *xraw }; // READ via T_x — does NOT kill T_xref in HB
    consume(xref); // FnEntry retag for T_xref → succeeds (T_xref still current_borrower)
    0
}
