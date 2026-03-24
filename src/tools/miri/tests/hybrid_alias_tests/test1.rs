#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 10; 
    let ref1 = &mut x;
    let ref2 = &mut *ref1;
    let ptr2 = ref2 as *mut i32;

    unsafe { *ptr2 += 1; }
    *ref2 += 1;
    
    unsafe {*ptr2 += 1; }
    
    *ref1 += 1; 

    unsafe {*ptr2 += 1; }

    x += 1;
    0
}