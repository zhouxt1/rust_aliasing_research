#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

pub fn test1() {
    let x = 20;
    let y = &x;
    let z = *y + 1;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 10; 
    let ref1 = &mut x;
    let sref1 = &*ref1;
    let sref2 = &*sref1;

    // essentially, since x is already dead, this shared loan is not marked as returned. 
    // Obviously, it is 'returned' but overwritten by the return of 'ref1 to x'. 
    // Only the first loan is 'returned'
    let mut y = *sref2 + *sref1 + *ref1;
    x += 1;

    let y2 = &mut y;

    let sref3 = &*y2;
    let sref4 = &*y2;
    let sref5 = &*sref3;

    let z = *sref5 + 1;
    let z1 = *sref4 + 1;
    // there is still a bug here

    *y2 += 1;

    test1();

    0
}