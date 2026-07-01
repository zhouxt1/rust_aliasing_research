#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

extern "Rust" {
    fn miri_alloc(size: usize, align: usize) -> *mut u8;
    fn miri_dealloc(ptr: *mut u8, size: usize, align: usize);
}

#[inline(never)]
unsafe fn inner(x: &mut i32) {
    // FnEntry retag → x is strongly protected for the duration of this call.
    // Dealloc through a pointer derived from x should be UB.
    let raw = x as *mut i32 as *mut u8;
    miri_dealloc(raw, 4, 4); //~ ERROR: strongly protected
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
