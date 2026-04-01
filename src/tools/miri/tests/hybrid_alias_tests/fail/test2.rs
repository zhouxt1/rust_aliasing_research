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
    let raw2 = ref1 as *mut i32;

    let sref1 = &*ref1;

    unsafe { *raw2 += 1; }
    let y = *sref1 + 1;
    0
}