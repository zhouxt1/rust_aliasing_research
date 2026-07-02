#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of 4 of the 6 sub-functions in pass/tree_borrows/tree-borrows.rs.
// `string_as_mut_ptr` is NOT ported: it needs `alloc::string::String`, which shares `Box`'s
// `RawVec`/allocator-lang-item machinery, known to hit a Polonius-MIR-for-stdlib bug.
// `aliasing_read_only_mutable_refs` is NOT here either — it genuinely fails in HB, see
// hb_tree_borrows_sibling_reborrow.rs.
// TB: pass on all 4 here.
// HB: pass — confirmed for all 4.

// Two mutable references from the same allocation both under a protector must not be UB.
fn two_mut_protected_same_alloc() {
    fn write_second(_x: &mut u8, y: &mut u8) {
        *y = 1;
    }

    let mut data = (0u8, 1u8);
    write_second(&mut data.0, &mut data.1);
}

// A reborrowed mutable reference returned from a function is actually writeable, despite the
// implicit read inserted on function exit.
fn returned_mut_is_usable() {
    fn reborrow(x: &mut u8) -> &mut u8 {
        let y = &mut *x;
        *y = *y;
        y
    }
    let mut data = 0;
    let x = &mut data;
    let y = reborrow(x);
    *y = 1;
}

// Coercing &mut T to *const T produces a writeable pointer.
fn direct_mut_to_const_raw() {
    let x = &mut 0;
    let y: *const i32 = x;
    unsafe {
        *(y as *mut i32) = 1;
    }
    assert_eq!(*x, 1);
}

#[allow(unused_assignments)]
fn local_addr_of_mut() {
    let mut local = 0;
    let ptr = core::ptr::addr_of_mut!(local);
    local = 1;
    unsafe { *ptr = 2 };
    local = 3;
    unsafe { *ptr = 4 };
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    two_mut_protected_same_alloc();
    direct_mut_to_const_raw();
    local_addr_of_mut();
    returned_mut_is_usable();
    0
}
