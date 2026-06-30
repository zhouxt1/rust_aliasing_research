#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Returning an `Option<&mut T>` whose inner ref was only shadowed by a parent READ is valid.
///
/// ## Source
/// Ported from `stacked_borrows/fail/return_invalid_mut_option.rs` — that test **fails** SB
/// but **passes** HB.
///
/// ## What this tests
///
/// Same mechanism as `sb_return_reserved_ref_after_parent_read.rs`, but the `&mut` is
/// wrapped in an `Option`. The key access is `*xraw` (a READ via base_pointer T_xraw).
///
/// In SB this read pops the child Unique `ret` from the stack. In HB parent reads via the
/// base_pointer do NOT kill the child entry: `ret` (T_ret) remains current_borrower when
/// the `Option<&mut i32>` is returned. The caller can unwrap and use it.
///
/// ## HB vs SB
///
/// SB: the `*xraw` read pops T_ret's Unique item → returning `Some(ret)` retaggs T_ret
/// at the return site → T_ret not in stack → UB.
///
/// HB: T_ret still in the exposed stack as current_borrower → return retag succeeds →
/// caller receives a valid `Some(&mut i32)`.
fn foo(x: &mut (i32, i32)) -> Option<&mut i32> {
    let xraw = x as *mut (i32, i32);
    let ret = unsafe { &mut (*xraw).1 }; // RawPtr reborrow: T_ret; exposed_stack = [{T_xraw, T_ret}]
    let ret = Some(ret);
    let _val = unsafe { *xraw }; // READ via T_xraw (base_pointer) — does NOT kill T_ret in HB
    ret // T_ret still current_borrower → return succeeds
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    match foo(&mut (1, 2)) {
        Some(r) => {
            *r = 99;
        }
        None => {}
    }
    0
}
