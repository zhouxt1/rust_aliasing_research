#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::mem;

// Port of fail/stacked_borrows/illegal_write4.rs
// SB: fail — creating a raw-tagged &mut via transmute from a raw pointer unfreezes a frozen
// location, invalidating a subsequent shared-reference read.
// HB: fail — confirmed, via the same mechanism as hb_static_memory_modification.rs: HB's
// retag rejects creating a non-shared (`&mut`) reference from a tag currently in shared-borrow
// state ("Attempting to create a non-shared reference using a shared tag"), eagerly at the
// transmute, before any read through `reference` is even attempted.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut target = 42;
    let raw = &mut target as *mut _;
    let reference = unsafe { &*raw };
    let _ptr = reference as *const _ as *mut i32;
    let _mut_ref: &mut i32 = unsafe { mem::transmute(raw) };
    let _val = *reference;
    0
}
