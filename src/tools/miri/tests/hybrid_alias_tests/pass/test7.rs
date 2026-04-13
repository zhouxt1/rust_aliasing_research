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

// Option::get_or_insert internally does:
//   pub fn get_or_insert(&mut self, value: T) -> &mut T {
//       if let None = *self { *self = Some(value); }
//       match self {              // <-- reborrow of &mut self
//           Some(v) => v,         // <-- returns reborrowed &mut T
//           ...
//       }
//   }
fn test_option_get_or_insert() {
    let mut opt: Option<i32> = None;
    let inner_ref = opt.get_or_insert(99);
    *inner_ref += 1;
    if opt != Some(100) {
        loop {}
    }
}

// Option::insert internally does:
//   pub fn insert(&mut self, value: T) -> &mut T {
//       *self = Some(value);
//       match self {              // <-- reborrow of &mut self after writing
//           Some(v) => v,         // <-- returns reborrowed &mut T
//           ...
//       }
//   }
fn test_option_insert() {
    let mut opt: Option<i32> = Some(1);
    let inner_ref = opt.insert(42);
    *inner_ref += 8;
    if opt != Some(50) {
        loop {}
    }
}

// slice::split_first_mut internally does:
//   pub fn split_first_mut(&mut self) -> Option<(&mut T, &mut [T])> {
//       let (first, rest) = self.split_at_mut(1);  // <-- reborrow self into two disjoint &mut
//       Some((&mut first[0], rest))                 // <-- further reborrow of first
//   }
fn test_split_first_mut() {
    let mut data = [5, 6, 7, 8];
    if let Some((first, rest)) = data.split_first_mut() {
        *first += 10;
        // rest is also a reborrow from the same original slice
        if let Some((second, _)) = rest.split_first_mut() {
            *second += 20;
        }
    }
    if data != [15, 26, 7, 8] {
        loop {}
    }
}

// slice::swap internally does:
//   pub fn swap(&mut self, a: usize, b: usize) {
//       let pa = ptr::addr_of_mut!(self[a]);  // <-- borrows self
//       let pb = ptr::addr_of_mut!(self[b]);  // <-- reborrows self
//       unsafe { ptr::swap_nonoverlapping(pa, pb, 1); }
//   }
fn test_slice_swap() {
    let mut data = [10, 20, 30];
    data.swap(0, 2);
    if data != [30, 20, 10] {
        loop {}
    }
}

// Option::replace internally does:
//   pub fn replace(&mut self, value: T) -> Option<T> {
//       mem::replace(self, Some(value))  // <-- reborrows &mut self into mem::replace
//   }
// and mem::replace internally does:
//   pub fn replace(dest: &mut T, src: T) -> T {
//       unsafe {
//           let result = ptr::read(dest);   // <-- borrows dest
//           ptr::write(dest, src);           // <-- reborrows dest
//           result
//       }
//   }
fn test_option_replace() {
    let mut opt = Some(5i32);
    let old = opt.replace(10);
    if old != Some(5) || opt != Some(10) {
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
    //test_slice_split_at_mut();  // fail
    // test_slice_iter_mut_next(); // fail
    // test_option_get_or_insert(); // pass
    test_option_insert();
    //test_split_first_mut(); // fail
    // test_slice_swap();
    // test_option_replace();
    0
}
