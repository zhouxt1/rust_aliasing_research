#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::mem;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

fn test_core_mem_replace() {
    let mut value = 10;
    let old = mem::replace(&mut value, 20);

    if old != 10 || value != 20 {
        loop {}
    }
}

fn test_core_mem_swap() {
    let mut left = 1;
    let mut right = 2;

    mem::swap(&mut left, &mut right);

    if left != 2 || right != 1 {
        loop {}
    }
}

fn test_core_mem_take() {
    let mut value = 7;
    let old = mem::take(&mut value);

    if old != 7 || value != 0 {
        loop {}
    }
}

fn test_option_as_mut() {
    let mut value = Some(11);

    if let Some(inner) = value.as_mut() {
        *inner += 1;
    } else {
        loop {}
    }

    if value != Some(12) {
        loop {}
    }
}

fn test_slice_first_mut() {
    let mut data = [3, 4, 5];

    if let Some(first) = data.first_mut() {
        *first += 10;
    } else {
        loop {}
    }

    if data != [13, 4, 5] {
        loop {}
    }
}

fn test_slice_split_at_mut() {
    let mut data = [1, 2, 3, 4];
    let (left, right) = data.split_at_mut(2);

    left[0] += 10;
    right[0] += 20;

    if data != [11, 2, 23, 4] {
        loop {}
    }
}

fn test_slice_iter_mut_next() {
    let mut data = [8, 9, 10];
    let mut it = data.iter_mut();

    if let Some(first) = it.next() {
        *first += 1;
    } else {
        loop {}
    }

    if let Some(second) = it.next() {
        *second += 2;
    } else {
        loop {}
    }

    if data != [9, 11, 10] {
        loop {}
    }
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    // test_core_mem_replace(); // pass
    // test_core_mem_swap(); // pass 
    // test_core_mem_take(); // pass
    // test_option_as_mut(); // pass 
    // test_slice_first_mut(); // pass
    test_slice_split_at_mut();  // fail
    // test_slice_iter_mut_next(); // fail
    0
}
