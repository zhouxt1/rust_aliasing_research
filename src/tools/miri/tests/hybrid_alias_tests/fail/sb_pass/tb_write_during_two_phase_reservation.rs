#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A foreign write during a two-phase reservation kills the Reserved borrow.
///
/// ## Source
/// Ported from `tree_borrows/fail/write-during-2phase.rs`.
///
/// ## What this tests
///
/// `f.add(...)` involves a two-phase borrow of `f`:
/// 1. At the call site, a `Reserved` reborrow of `f` is created for the `&mut self`
///    argument (tag T_self, current_borrower = T_self, shared_borrower = T_alias for
///    the field previously borrowed).
/// 2. The argument expression `unsafe { *alias = 42; 0 }` is evaluated — this is a
///    foreign write via T_alias while T_self is in `Reserved` state.
/// 3. `Reserved` does NOT tolerate foreign writes (only `ReservedIM` does, for `!Freeze`
///    types). The write is denied.
///
/// ## HB vs SB
///
/// SB: this pattern passes Stacked Borrows. SB's two-phase handling allows the shared
/// borrow in the argument to coexist with the reservation.
///
/// TB: fails at reborrow-from-Reserved inside the callee (`self.0 + n`), because the
/// Reserved state was damaged.
///
/// HB: fails at `*alias = 42` — the write to a `Reserved` (Freeze type) allocation
/// is denied immediately.
///
/// ## Freeze vs !Freeze
///
/// `u64` is a `Freeze` type, so the 2-phase borrow uses `Reserved` (not `ReservedIM`).
/// If the struct field were `Cell<u64>` (!Freeze), `ReservedIM` would tolerate the
/// foreign write and the test would pass. See `pass/two_phase_not_freeze_reservedim.rs`.
struct Foo(u64);

impl Foo {
    fn add(&mut self, n: u64) -> u64 {
        self.0 + n
    }
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut f = Foo(0);
    let alias = &mut f.0 as *mut u64; // alias carries T_f0, current_borrower of f.0

    // f.add(...):
    //   1. Reserve &mut f → T_self (Reserved, shared_borrower = T_f0)
    //   2. Evaluate argument: *alias = 42 — foreign write while Reserved
    //      HB: denied (Freeze type → Reserved, not ReservedIM)  //~ ERROR
    //   3. Would activate T_self on fn entry (never reached)
    let _res = f.add(unsafe {
        *alias = 42; //~ ERROR: foreign write during Reserved 2-phase borrow (Freeze type)
        0
    });

    0
}
