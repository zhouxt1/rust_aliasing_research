#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::UnsafeCell;

// REGRESSED (2026-07-02): moved here from pass/all/ after the `RawPointerStack.dead` fix (see
// COVERAGE.md's "pass/sb_tb_fail case study"). HB now fails these, because the fix kills a
// `RawPointerStack` entry on ANY read through its base pointer, without distinguishing TB's
// Reserved (never self-written) vs Active (already self-written) states. All 4 functions below
// create `x` and never write through it before a sibling reborrow from `base` is treated as a
// base-pointer read, so `x`'s entry dies even though TB would tolerate this (Reserved nodes
// survive foreign reads/writes). Fixing this requires the deferred `activated: bool` field —
// only kill on parent read once the entry has actually been self-written.
//
// Port of 4 of the 6 sub-functions in pass/tree_borrows/reserved.rs (utils diagnostic macros /
// eprintln! stripped). The other 2 (cell_protected_read, int_protected_read) genuinely diverge
// in HB — see hb_reserved_protected_conflict.rs.
// TB: pass — Foreign Read/Write on an unprotected Reserved (interior-mutable or not) is a noop,
// must not cause immediate UB.
// HB: **fail** (post-fix, regression) — see note above.

// Foreign Read on an interior mutable pointer is a noop.
unsafe fn cell_unprotected_read() {
    let base = &mut UnsafeCell::new(0u64);
    let x = &mut *(base as *mut UnsafeCell<_>);
    let _ = &x; // matches upstream's diagnostic touch of x, which happens before y is created
    let y = &mut *base as *mut UnsafeCell<u64> as *mut u64;
    let _val = *y;
}

// Foreign Write on an interior mutable pointer is a noop.
unsafe fn cell_unprotected_write() {
    let base = &mut UnsafeCell::new(0u64);
    let x = &mut *(base as *mut UnsafeCell<u64>);
    let _ = &x;
    let y = &mut *base as *mut UnsafeCell<u64> as *mut u64;
    *y = 1;
}

// Foreign Read on a Reserved is a noop.
unsafe fn int_unprotected_read() {
    let base = &mut 0u8;
    let x = &mut *(base as *mut u8);
    let _ = &x;
    let y = (&mut *base) as *mut u8;
    let _val = *y;
}

// Foreign Write on a Reserved turns it Disabled.
unsafe fn int_unprotected_write() {
    let base = &mut 0u8;
    let x = &mut *(base as *mut u8);
    let _ = &x;
    let y = (&mut *base) as *mut u8;
    *y = 1;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        cell_unprotected_read();
        cell_unprotected_write();
        int_unprotected_read();
        int_unprotected_write();
    }
    0
}
