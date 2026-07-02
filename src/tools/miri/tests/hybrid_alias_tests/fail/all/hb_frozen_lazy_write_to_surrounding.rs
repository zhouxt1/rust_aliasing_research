#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/frozen-lazy-write-to-surrounding.rs
// TB: fail — the "inside" part (`pair.1`) is `!Freeze` conceptually adjacent to the ZST field;
// widening a pointer derived from the frozen ZST field to write 4 bytes is forbidden.
// HB: candidate covered (not a per-byte divergence) — HB's BorrowerState is per-allocation, so
// `&pair.0` should set the WHOLE `pair` allocation to Frozen/Read permission, and the write
// through the widened pointer should be denied by that same allocation-level Frozen state,
// regardless of per-byte granularity. Confirm by running.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let pair = ((), 1i32);
    let x = &pair.0;
    let ptr = (core::ptr::addr_of!(*x) as *const i32) as *mut i32;
    unsafe { ptr.write(0) }; //~ ERROR: write access
    0
}
