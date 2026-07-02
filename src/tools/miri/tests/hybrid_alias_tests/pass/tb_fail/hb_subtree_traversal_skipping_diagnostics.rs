#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/subtree_traversal_skipping_diagnostics.rs
// TB: fail — a read through `other_ptr` is not forwarded past a Frozen intermediary node to
// the subtree below it (a traversal-skipping optimization), so `m` incorrectly stays Reserved
// and the subsequent write through `m` is forbidden.
// HB: pass — confirmed. HB has no tree/subtree structure at all, so there is no "traversal
// skipping" concept to misbehave; the read through `other_ptr` has no effect on `m`'s
// permission, and the write through `m` succeeds.

fn write_to_mut(m: &mut u8, other_ptr: *const u8) {
    unsafe {
        core::hint::black_box(*other_ptr);
    }
    *m = 42;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let root = 42u8;
    unsafe {
        let intermediary = &root;
        let data = &mut *(core::ptr::addr_of!(*intermediary) as *mut u8);
        write_to_mut(data, core::ptr::addr_of!(root));
    }
    0
}
