#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/illegal_dealloc1.rs
// SB: fails — ptr2's tag is popped from the borrow stack when ptr1.write(0) is executed,
//     so deallocating through ptr2 gives "tag does not exist in the borrow stack".
// HB: passes — HB has no "pop on lower access" semantics. Writing through ptr1 does not
//     invalidate ptr2. The allocation still has ptr2's tag visible in BorrowerState, and
//     before_memory_deallocation finds no StrongProtector, so dealloc succeeds.

extern "Rust" {
    fn miri_alloc(size: usize, align: usize) -> *mut u8;
    fn miri_dealloc(ptr: *mut u8, size: usize, align: usize);
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let x = miri_alloc(1, 1);
        let ptr1 = (&mut *x) as *mut u8;
        let ptr2 = (&mut *ptr1) as *mut u8;
        // SB: this write through ptr1 pops ptr2 off the borrow stack.
        // HB: no pop-on-lower-access — ptr2 remains valid.
        ptr1.write(0);
        miri_dealloc(ptr2, 1, 1);
    }
    0
}
