#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}



fn optimize_me(safe_ref: &mut i32, raw_ptr: *mut i32) -> i32 {
    let val1 = *safe_ref;      // Load 1
    unsafe { *raw_ptr = 42; }  
    // This invalidates the program. However, our system cannot catch it for now. 
    val1
}

/// Now we want to test something else. We want to know
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {

    let mut x = 10; 
    let y = &mut x;
    let raw1 = y as *mut i32;

    //let z = unsafe { &mut *raw1 }; // ok i see the problem, since y is gone after last use in the previous line, it fails to use the 'prev borrower

    unsafe { 

        let z = &mut *raw1; 
        optimize_me(z, raw1); 
    }
    

    0
}