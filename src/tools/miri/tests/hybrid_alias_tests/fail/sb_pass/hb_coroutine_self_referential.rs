#![no_std]
#![no_main]
#![feature(coroutines, coroutine_trait, stmt_expr_attributes)]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::ops::{Coroutine, CoroutineState};
use core::pin::Pin;

// Port of pass/stacked_borrows/coroutine-self-referential.rs
// SB: pass, but "fails when Stacked Borrows is strictly applied even to !Unpin types" per the
// upstream comment — the specific SB implementation tolerates this even though it's a
// self-referential mutable borrow held live across yield points.
// HB: fail — confirmed. `Pin` is `core`, and `Coroutine`/`CoroutineState` are available in
// `core::ops` on this nightly, so it compiles fine under `#![no_std]` and runs quickly (no
// hang — the coroutine's desugared resume/yield state machine is not itself a problem for HB).
// The actual failure: each `resume()` call re-executes `let num = &mut num;` from the
// coroutine's saved state, and `reborrow_chain` accumulates a new entry every time (observed
// growing to 16 entries across 3 resumes) without ever being cleared between yields. Eventually
// an access with a tag from an earlier resume no longer matches `current_borrower` or the
// (single-slot) `prev_borrower`. This looks like a real gap in how HB's reborrow-chain
// bookkeeping interacts with resumable coroutine state machines — a different MIR shape than
// anything else in this corpus (ordinary function calls don't re-execute the same retag
// statement repeatedly against accumulating prior state).

fn firstn() -> impl Coroutine<Yield = u64, Return = ()> {
    #[coroutine]
    static move || {
        let mut num = 0;
        let num = &mut num; //~ ERROR: neither current nor prev borrower exists

        yield *num;
        *num += 1;

        yield *num;
        *num += 1;

        yield *num;
        *num += 1;
    }
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut coroutine_iterator = firstn();
    let mut pin = unsafe { Pin::new_unchecked(&mut coroutine_iterator) };
    let mut sum = 0;
    while let CoroutineState::Yielded(x) = pin.as_mut().resume(()) {
        sum += x;
    }
    if sum == 3 { 0 } else { 1 }
}
