#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

pub fn test1() {
    let mut x = 10; 
    let mut i = 1;
    x += i;
    let ref2 = &mut x;
    *ref2 += 1;
    x += 1;
    i += 1;
}

pub fn test2() {
    let mut x = 10; 
    let mut i = 1;
    while i < 3 {
        x += i;
        let ref2 = &mut x;
        *ref2 += 1;
        i += 1;
    }
}


#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {

    test2();

    0
}