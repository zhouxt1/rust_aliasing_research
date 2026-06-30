#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test references created from raw pointers. 
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 10; 
    let ref1 = &mut x; 
    let raw1 = ref1 as *mut i32;

    let ref2 = unsafe { &mut *raw1 }; // exposed stack should add ref2. 
    *ref2 += 1; 

    *ref1 += 1; 
    //*ref2 += 1;

    0
}