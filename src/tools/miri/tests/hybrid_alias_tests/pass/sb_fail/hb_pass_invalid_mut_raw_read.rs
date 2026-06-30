#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// An inline parent-raw READ does not invalidate a child `&mut` for function passing in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/pass_invalid_mut.rs` — that test **fails** SB but
/// **passes** HB.
///
/// ## What this tests
///
/// In SB, any use of `xraw` (the parent raw pointer) after `xref = &mut *xraw` was created
/// pops `xref`'s Unique item from the stack. So `*xraw` (read) invalidates `xref`, and
/// then passing `xref` to `foo` triggers a FnEntry retag that fails because `xref`'s tag
/// no longer exists in the borrow stack.
///
/// In HB, a READ through the base-pointer (T_xraw) of the exposed_stack entry does not
/// truncate the stack: `check_raw_pointer_stack` returns `Ok(())` and leaves T_xref as
/// the top-of-stack `current_borrower`. When `foo(xref)` is called, the FnEntry retag
/// sees T_xref as the effective current borrower and succeeds.
///
/// ## Key difference from `sb_raw_read_doesnt_kill_child_mut.rs`
///
/// In `sb_raw_read_doesnt_kill_child_mut.rs` the base-pointer read is performed by a
/// **callee** that receives `xraw` as an argument. Here the read is performed **inline**
/// in the same scope, and the child `xref` is then passed *as an argument* to another
/// function — testing a different call-boundary scenario.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops `xref`'s Unique item; FnEntry retag fails |
/// | TB    | pass    | TB: `xref` is Reserved, parent read does not disable Reserved |
/// | HB    | pass    | Base-pointer READ does not truncate exposed_stack |
#[inline(never)]
fn foo(_x: &mut i32) {
    // Just consume the reference — verifies FnEntry retag succeeds.
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = &mut 42i32; // T_x = current_borrower
    let xraw = x as *mut i32; // Raw retag: T_xraw; exposed_stack = [(T_x, T_xraw)]
    let xref = unsafe { &mut *xraw }; // RawPtr reborrow: T_xref;
                                       // exposed_stack = [(T_x, T_xraw), (T_xraw, T_xref)]

    let _val = unsafe { *xraw }; // READ via T_xraw (base_pointer of xref's exposed_stack entry)
                                  // SB: pops xref's Unique item  HB: does not kill xref

    foo(xref); // FnEntry retag: SB errors (T_xref gone); HB succeeds (T_xref still top of stack)

    0
}
