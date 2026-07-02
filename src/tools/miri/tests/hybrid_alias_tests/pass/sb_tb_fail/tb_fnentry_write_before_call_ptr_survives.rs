#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// STILL UNFIXED (2026-07-02) after the `RawPointerStack.dead` fix — see COVERAGE.md's
/// "pass/sb_tb_fail case study" for why. This test's mechanism is not even a parent-read case:
/// `z = &mut x as *mut i32` is a bare cast (per `mod.rs`'s "raw pointers are only retagged for
/// `RetagKind::Raw`" comment), so `z` carries `T_z = current_borrower` directly — no
/// `RawPointerStack` entry ever exists here for the `dead` flag to act on. The actual gap is
/// that HB's plain `current_borrower`/`prev_borrower` displacement mechanism (used for FnEntry
/// retags) never disables/freezes a displaced `prev_borrower` on a foreign write the way TB's
/// Active→Disabled transition does. Fixing this would mean changing how FnEntry retags
/// interact with `prev_borrower`, independent of anything in the `RawPointerStack` fix.
///
/// Writing through a raw pointer, calling a function that reborrows it, then writing again
/// is valid in HB. Tree Borrows fails this; Stacked Borrows also fails it.
///
/// ## Source
/// Ported from `tree_borrows/fail/fnentry_invalidation.rs` — that test **fails** TB
/// (and SB) but **passes** HB.
///
/// ## Difference from sb_fnentry_doesnt_invalidate_raw.rs
///
/// The SB fnentry test (`pass/stacked_borrows/sb_fnentry_doesnt_invalidate_raw.rs`) does
/// NOT write through `z` before the call. This test DOES write `*z = 1` first, which
/// activates z as a "Unique/Active" node in Tree Borrows. The subsequent FnEntry retag
/// from `do_bad()` is then a "Foreign Write" for z's Active TB node, causing it to
/// transition to Disabled — so the second write `*z = 2` fails in TB.
///
/// ## Why HB passes this
///
/// In HB, the write `*z = 1` just uses T_z as current_borrower. The FnEntry retag from
/// `do_bad()` mints T_self (new current_borrower) and pushes T_z to prev_borrower. After
/// `do_bad()` returns (with an empty body), the return-borrower machinery restores
/// T_z as current_borrower. The second write `*z = 2` uses T_z (current_borrower) → OK.
///
/// HB never freezes/disables previous borrowers on FnEntry retag; it only displaces them
/// temporarily. Only an explicit WRITE reclaims the borrow permanently.
///
/// ## Model comparison
///
/// | Access        | SB     | TB       | HB    |
/// |---------------|--------|----------|-------|
/// | `*z = 1`      | OK     | OK       | OK    |
/// | `x.do_bad()`  | ❌ UB  | ❌ UB    | OK    |
/// | `*z = 2`      | —      | ❌ UB    | OK    |
trait Bad {
    fn do_bad(&mut self) {
        // empty body — no write through self
    }
}

impl Bad for i32 {}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 0i32;
    let z = &mut x as *mut i32; // T_z = current_borrower

    unsafe { *z = 1; } // WRITE via T_z (current) — activates z's Unique in TB; OK in HB

    // FnEntry retag: T_self = new current, T_z = prev_borrower.
    // Body is empty, so no write through self.
    // Return: T_z restored as current_borrower.
    x.do_bad();

    unsafe { *z = 2; } // WRITE via T_z (restored current_borrower) — TB: Disabled → UB; HB: OK

    0
}
