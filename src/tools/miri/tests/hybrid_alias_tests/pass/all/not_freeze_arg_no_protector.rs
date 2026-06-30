#![no_std]
#![no_main]

use core::cell::Cell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Phase 3 — `!Freeze` shared ref function argument must NOT receive a protector.
///
/// ## What this tests
///
/// When `&T` where `T: !Freeze` is passed as a function argument, Phase 3 suppresses
/// protector installation for it. If a `StrongProtector` were installed, any foreign
/// write through an aliased pointer during the call would be flagged as UB — but writes
/// through `Cell`/`UnsafeCell` aliases are valid by definition.
///
/// The pattern here:
/// 1. `c: Cell<i32>` with a raw alias `raw` pointing to the same memory.
/// 2. `c` is passed by shared ref to `read_and_alias_write`.
/// 3. Inside the callee, write through `raw` (a foreign write).
/// 4. This must succeed — `&Cell<i32>` is `!Freeze`, so no protector is installed.
///
/// ## Contrast with fail/test4.rs
///
/// `fail/test4.rs` passes `&i32` (`Freeze`) + a raw alias → the `StrongProtector` on
/// the `&i32` makes the foreign write UB. Here the reference is `&Cell<i32>` (`!Freeze`)
/// → no protector → write is allowed.

#[inline(never)]
fn read_and_alias_write(cell_ref: &Cell<i32>, raw: *mut i32) -> i32 {
    let before = cell_ref.get();
    unsafe { *raw = 99; }       // foreign write during the call — valid because !Freeze
    let after = cell_ref.get();
    (before + after) as i32
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let c = Cell::new(0i32);
    let raw: *mut i32 = c.as_ptr();

    let _result = read_and_alias_write(&c, raw);

    0
}
