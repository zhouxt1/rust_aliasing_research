#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/deallocate_against_protector1.rs
// SB: fails — deallocating through a raw pointer derived from a strongly-protected &mut.
// HB: fails — FnEntry retag for `inner` creates StrongProtector on tag T_inner, which
//     lands in exposed_stack[0].current_borrower. When `callback` calls miri_dealloc,
//     before_memory_deallocation scans exposed_stack and finds T_inner is protected.

extern "Rust" {
    fn miri_alloc(size: usize, align: usize) -> *mut u8;
    fn miri_dealloc(ptr: *mut u8, size: usize, align: usize);
}

// Callback type: may mutate, but must not deallocate.
// HB: when inner calls callback(x), x already carries inner's FnEntry protector.
#[inline(never)]
unsafe fn callback(x: &mut i32) {
    let raw = x as *mut i32 as *mut u8;
    miri_dealloc(raw, 4, 4); //~ ERROR: strongly protected
}

#[inline(never)]
unsafe fn inner(x: &mut i32) {
    callback(x);
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let ptr = miri_alloc(4, 4) as *mut i32;
        core::ptr::write(ptr, 0i32);
        inner(&mut *ptr);
    }
    0
}
