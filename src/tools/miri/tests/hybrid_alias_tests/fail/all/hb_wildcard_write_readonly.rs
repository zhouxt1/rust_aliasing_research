#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/exposed_only_ro.rs
// SB: fail — only a read-only (SharedReadOnly) tag was exposed; write through wildcard denied.
// HB: fail — only the shared reference's tag is in `exposed_tags`. `resolve_wildcard_tag` finds
//     that tag live (in `prev_borrower`/`shared_borrower` depending on the exact reborrow
//     state), but it is neither `current_borrower` nor `prev_borrower` by the time of the
//     write, so `check_borrower_tag` denies the access — converging with SB's verdict, even
//     though SB and HB reach it via different bookkeeping (permission-class vs identity check).

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 0i32;
    let _fool = &mut x as *mut i32; // mutable raw (would fool old untagged logic)
    unsafe {
        // Expose provenance of the shared reference (read-only tag in SB).
        let addr = (&x as *const i32).expose_provenance();
        let wild = core::ptr::with_exposed_provenance_mut::<i32>(addr);
        // SB: denied (shared tag has no write permission).
        // HB: denied (the exposed shared-ref tag is no longer the live owner identity).
        *wild = 0; //~ ERROR: Access denied
    }
    0
}
