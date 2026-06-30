#![no_std]
#![no_main]

use core::mem;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A shared reference is invalidated when the raw pointer it was created alongside is written.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read8.rs` (std::mem → core::mem).
///
/// ## What this tests
///
/// ```
/// let x = &mut 0i32;
/// let y1: &i32 = mem::transmute(&*x);  // shared ref with laundered lifetime
/// let y2 = x as *mut _;                // raw pointer carrying T_x
/// let _val = *y2;                      // read via raw
/// let _val = *y1;                      // read via shared ref
/// *y2 += 1;                            // WRITE while allocation is in Read state → UB in HB
/// let _fail = *y1;                     // SB/TB: UB here; HB: UB already above
/// ```
///
/// After `y1 = &*x` (shared reborrow), HB transitions the allocation to
/// `BorrowerPermission::Read`. The subsequent write `*y2 += 1` is denied immediately
/// because `Read` state unconditionally rejects writes.
///
/// ## HB vs SB
///
/// SB: `*y2 += 1` pops the SharedReadOnly (y1) from the stack because a write via a Unique
/// tag kills items above it. The error is triggered at the final read `*y1` (y1's tag is gone).
///
/// HB: error is at `*y2 += 1` (the write itself is denied in Read state). HB catches the
/// UB earlier than SB.
///
/// ## Note on mem::transmute
///
/// `mem::transmute` is used to launder the lifetime of the shared ref (simulating code where
/// a `&i32` with some lifetime is held while the original `&mut i32` is also alive). This is a
/// standard Miri test technique. `core::mem::transmute` is the exact same function.
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let x = &mut 0i32;
        // Launder the lifetime so y1 can outlive the normal borrow of x.
        // mem::transmute is in core — no std needed.
        let y1: &i32 = mem::transmute(&*x); // shared reborrow → BorrowerPermission::Read
        let y2 = x as *mut i32; // raw pointer carrying T_x (current_borrower at creation time)

        let _val = *y2; // read via T_x — currently current_borrower (Read state allows current reads?)
        let _val = *y1; // read via shared_borrower T_shr — OK in Read state

        *y2 += 1; //~ ERROR: write via T_x while allocation is in Read state → denied
        let _fail = *y1;
    }
    0
}
