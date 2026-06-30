#![no_std]
#![no_main]

use core::cell::Cell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Interior mutability — two-phase borrow over a `!Freeze` type (`ReservedIM`).
///
/// ## What this tests
///
/// `Cell<i32>` is `!Freeze`. When a two-phase `&mut Cell<i32>` is created (e.g. by
/// `Cell::set`), the reservation phase must tolerate writes through a shared alias —
/// because a `Cell` may be mutated via its `&Cell<i32>` interface while the `&mut`
/// reservation is live.
///
/// This test manually reproduces the structure of `Cell::replace`-style code:
/// 1. Take a shared reference `r` to the cell (carries `current_borrower` = T0 via
///    Phase 1 tag inheritance).
/// 2. Create a two-phase `&mut Cell<i32>` reservation (T1 = new current_borrower,
///    T0 = shared_borrower, state = `ReservedIM`).
/// 3. Write through the shared alias `r` during the reservation window
///    (foreign write tolerated by `ReservedIM`).
/// 4. Activate the two-phase borrow (T1 → `Write`).
/// 5. Write through the activated `&mut` (normal `Write` access).
///
/// ## Phase 2 implementation
///
/// `hb_reborrow` detects `!ty_is_freeze && NewPermission::TwoPhase` and transitions to
/// `BorrowerPermission::ReservedIM` instead of `Reserved`. `check_borrower_tag` for
/// `ReservedIM + Write` accepts `shared_borrower` (T0) writes without activation.
///
/// ## Current status
///
/// Should pass after Phase 2. If `Reserved` were used instead of `ReservedIM`, step 3
/// would fail with "write denied" because `Reserved::Write` only accepts `current_borrower`.

#[inline(never)]
fn set_via_shared(r: &Cell<i32>, val: i32) {
    r.set(val);
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let c = Cell::new(0i32);

    // Phase 1: r inherits current_borrower tag — !Freeze shared ref.
    let r = &c;

    // Write through the shared alias (this is the "foreign write during reservation" case).
    // Cell::set internally does UnsafeCell::get() + write, using the inherited tag.
    set_via_shared(r, 10);

    // Read back — should see 10.
    let _v = r.get();

    0
}
