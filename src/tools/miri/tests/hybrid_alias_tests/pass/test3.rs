#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


// testing function calls and retag. 
pub fn test1() {
    let mut x = 20;
   // let x1 = &mut x; 
    test2(&mut x);
    // let y = *x1;
    // *x1 += 1;
    x += 1;
}

pub fn test2(ref1 : &mut i32) {
    *ref1 += 1;

    let ref2 = &mut *ref1;
    *ref2 += 1; // I don't think this was killed. But the issue is that 

   // *ref1 += 2;
}


// testing function calls and retag. 
pub fn test3() {
    let mut x = 20;
    let mut y = 15;
   // let x1 = &mut x; 
    test4(&mut x, &mut y);
    // let y = *x1;
    // *x1 += 1;
    x += 1;
    y += 1;
}

pub fn test4(ref1 : &mut i32, ref2 : &mut i32) {
    *ref1 += 1;
    *ref2 += 1; 

    let ref3 = *ref1 + *ref2; 

   // *ref1 += 2;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {

    //test3();
    test1();

    0
}