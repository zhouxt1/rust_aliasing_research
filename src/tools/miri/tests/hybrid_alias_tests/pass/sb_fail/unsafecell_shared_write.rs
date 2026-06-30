#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Interior mutability — basic `UnsafeCell` write via shared reference.
///
/// ## What this tests
///
/// `UnsafeCell<i32>` is `!Freeze`. Under Phase 1 tag inheritance, `&c` does not mint a
/// new `shared_borrower` tag or transition to `BorrowerPermission::Read`. Instead the
/// shared reference inherits the parent's `current_borrower` tag — giving it the same
/// write authority as the original allocation. Writes via `.get()` go through the normal
/// `Write` check and succeed.
///
/// ## Current status
///
/// **FAILS** until Phase 1 is implemented. Today `&UnsafeCell<i32>` creates a shared reborrow
/// that puts the allocation into `BorrowerPermission::Read`; the subsequent write is rejected
/// by `check_borrower_tag` with "Access denied: write access on a shared borrow".
///
/// ## After Phase 1
///
/// `hb_reborrow` detects `!ty.is_freeze()` for `NewPermission::Read` and returns `parent_tag`
/// unchanged. The allocation stays in `Write` state; the write via `.get()` succeeds.
///
/// ## Contrast
///
/// `fail/test7.rs` does the same write but through a plain `&i32` (a `Freeze` type).
/// `i32: Freeze` takes the existing `Read` path, and the write remains UB.

fn write_via_shared(c: &UnsafeCell<i32>, val: i32) {
    // UnsafeCell::get() is an inline pointer cast — no Polonius MIR needed.
    unsafe { *c.get() = val; }
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let c = UnsafeCell::new(0i32);

    // Take a shared reference — &UnsafeCell<i32> is !Freeze, so Phase 1 tag
    // inheritance returns parent_tag rather than transitioning to Read.
    let r = &c;
    write_via_shared(r, 42);

    // A second write through the direct cell ref: both accesses carry current_borrower
    // and go through the Write-state allocation, so both are valid.
    unsafe { *c.get() += 1; }

    0
}
