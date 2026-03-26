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

pub fn test2() {
    let mut x = 10; 
    let ref1 = &mut x;
    let sref1 = &*ref1;
    let sref2 = &*sref1;

    // essentially, since x is already dead, this shared loan is not marked as returned. 
    // Obviously, it is 'returned' but overwritten by the return of 'ref1 to x'. 
    // Only the first loan is 'returned'
    let mut y = *sref2 + *sref1 + *ref1;
    x += 1;
}

pub fn test3() {
    let mut y = 30;
    let y2 = &mut y;

    let sref3 = &*y2;
    let sref4 = &*y2;
    let sref5 = &*sref3;

    let z = *sref5 + 1;
    let z1 = *sref4 + 1;
    // there is still a bug here

    *y2 += 1;
    //test1();
}

// let's test function where a shared reference is killed. 
pub fn test4() {
    let mut y = 10;
    let mut z = 20;

    let mut y1 = &mut y; 

    let sref1 = &*y1;
    let sref2 = &*sref1;

    y1 = &mut z;

    let x = *sref1 + *sref2; 

}

// let's test reference that might have two 'origins'
pub fn test5() {
    let mut x = 10;
    let mut y = 10;

    let mut ref1 = if true {
        &mut x
    } else {
        &mut y
    };

    *ref1 += 1;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {

    //test4();
    test4();

    0
}