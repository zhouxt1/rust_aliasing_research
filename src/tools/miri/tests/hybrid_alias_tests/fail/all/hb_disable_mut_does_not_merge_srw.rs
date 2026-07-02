#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/disable_mut_does_not_merge_srw.rs
// SB: fail — "Disabling" mutref (via the write through base) does not merge SRW groups;
// the subsequent write through base pops raw (a different SRW group) off the stack.
// HB: fail — confirmed. The write through `base` invalidates `raw`'s tag; a subsequent access
// through `raw` finds no matching current/prev borrower.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let mut mem = 0;
        let base = &mut mem as *mut i32;
        let raw = {
            let mutref = &mut *base;
            mutref as *mut i32
        };
        let _val = *base;
        *base = 1;
        let _val = *raw;
    }
    0
}
