#![no_std]
#![no_main]

use core::cell::Cell;
use core::panic::PanicInfo;
use core::ptr;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Interior mutability — `Cell<i32>` writes via `.as_ptr()`.
///
/// ## What this tests
///
/// `Cell<T>` is the safe wrapper around `UnsafeCell<T>`. `Cell::as_ptr()` is defined as
/// `self.value.get()` — a trivial inline pointer cast, no complex Polonius MIR needed.
/// This test uses the same aliasing pattern as test10 but via `Cell` rather than raw
/// `UnsafeCell`, which is the more common real-world usage (e.g. `Rc` internals, thread-local
/// state, cyclic data structures).
///
/// `Cell<i32>` contains `UnsafeCell<i32>`, so its type is `!Freeze`. Under Phase 2 the
/// allocation receives `BorrowerPermission::Cell`, and the writes via `ptr::write` succeed.
///
/// ## Stdlib note
///
/// `Cell<T>` and `ptr::write` are both in `core` — no `std` needed. `Cell::as_ptr()` is an
/// `inline` / `const fn` that reduces to a pointer cast; it does not require pre-serialized
/// Polonius MIR for core the way `Option::insert` does (see test7.rs).
///
/// ## Current status
///
/// **FAILS** until Phase 1. The allocation is put into `BorrowerPermission::Read` when the
/// first `&Cell<i32>` is created; the subsequent `ptr::write` is a write access that is
/// denied.
///
/// ## After Phase 1
///
/// Both `&c` reborrows see `!Freeze` + `NewPermission::Read` → return `parent_tag`, no
/// state transition. The allocation stays in `Write`; `ptr::write` via either `.as_ptr()`
/// succeeds (tag = `current_borrower`).

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let c = Cell::new(0i32);

    // Two shared references — same pattern as test10, but through the Cell<T> abstraction.
    let r1 = &c;
    let r2 = &c;

    unsafe {
        // Cell::as_ptr() == self.value.get() (UnsafeCell::get), an inline pointer cast.
        ptr::write(r1.as_ptr(), 10);
        ptr::write(r2.as_ptr(), 20);

        // Reads back through either pointer are also valid.
        let _v = ptr::read(r1.as_ptr());
    }

    0
}
