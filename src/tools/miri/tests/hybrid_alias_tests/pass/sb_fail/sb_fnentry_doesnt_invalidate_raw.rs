#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// FnEntry retag of `x` does not invalidate a previously created raw pointer in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/fnentry_invalidation.rs` — that test **fails** SB
/// but **passes** HB (and Tree Borrows).
///
/// ## What this tests
///
/// `z = &mut x as *mut i32` creates a raw pointer with tag T_z = current_borrower.
///
/// `x.do_bad()` passes `&mut x` as `self`. FnEntry retag fires: HB mints a new tag
/// T_self (Ref reborrow), setting current_borrower = T_self and prev_borrower = T_z.
///
/// `do_bad` has an empty body — no accesses, no write. On return, HB's
/// return-borrower machinery (Polonius return anchors / `hb_before_statement`)
/// restores current_borrower = T_z (the caller's borrower is revived).
///
/// The post-call read `*z` uses T_z, which is now current_borrower again → succeeds.
///
/// ## Why SB fails this
///
/// In SB, FnEntry retag pushes a new Unique item for `self` on top of z's tag on the
/// borrow stack. Even though `do_bad` never writes through `self`, the retag itself
/// overwrites z's item. After the call, `*z` accesses a tag that no longer exists in
/// the stack → UB.
///
/// ## Key HB design choice
///
/// HB only requires the CURRENT_BORROWER to be active at access time. Since do_bad
/// returns without a write, the return-borrower machinery correctly restores T_z.
/// No write → no permanent invalidation of z.
///
/// Also see `tree_borrows/pass/sb_fails.rs :: fnentry_invalidation` — TB passes this
/// for the same reason: an actual write is required to make UB.
trait Bad {
    fn do_bad(&mut self) {
        // empty body — no write through self
    }
}

impl Bad for i32 {}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 0i32;
    let z = &mut x as *mut i32; // T_z = current_borrower

    // FnEntry retag for &mut self: T_self minted, T_z → prev_borrower.
    // On return: return-borrower machinery restores T_z as current_borrower.
    x.do_bad();

    unsafe {
        let _oof = *z; // T_z is current_borrower again → read succeeds
    }

    0
}
