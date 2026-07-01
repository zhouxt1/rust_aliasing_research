#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/strongly-protected.rs
// TB: fails — deallocation through raw pointer while &mut protector is still active.
// HB: fails — inner receives &mut i32 (FnEntry StrongProtector on T_inner), then passes
//     the allocation address as a raw pointer to callback. T_inner is still registered as
//     StrongProtector when callback calls miri_dealloc, so before_memory_deallocation
//     finds it in exposed_stack and errors.

extern "Rust" {
    fn miri_alloc(size: usize, align: usize) -> *mut u8;
    fn miri_dealloc(ptr: *mut u8, size: usize, align: usize);
}

// callback takes a raw pointer — no FnEntry retag for raw, so the only active
// protector is inner's &mut protector.
#[inline(never)]
unsafe fn callback(raw: *mut i32) {
    miri_dealloc(raw as *mut u8, 4, 4); //~ ERROR: strongly protected
}

#[inline(never)]
unsafe fn inner(x: &mut i32) {
    callback(x as *mut i32);
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
