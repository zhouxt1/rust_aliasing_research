#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A raw-pointer cast from a `&mut` ref remains readable via `prev_borrower` after a
/// shared reborrow of that ref is active.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read6.rs` — that test **fails** SB but
/// **passes** HB.
///
/// ## What this tests
///
/// In SB:
///   1. `x: &mut 0` — Unique(T_x) on the borrow stack for the allocation.
///   2. `raw = x as *mut _` — pushes SRW(T_raw) on top: stack = [Unique(T_x), SRW(T_raw)].
///   3. `x2 = &mut *x` — Unique reborrow from T_x. Creating a Unique item from T_x pops
///      everything ABOVE T_x, so SRW(T_raw) is removed. Stack = [Unique(T_x), Unique(T_x2)].
///   4. `_y = &*x2` — creates SRO on top. Stack = [Unique(T_x), Unique(T_x2), SRO(T_y)].
///   5. `*raw` (read via T_raw) — T_raw is NOT in the stack → ERROR.
///
/// In HB:
///   1. `x` has T_x as current_borrower.
///   2. `raw = x as *mut _` — Raw retag from T_x, creating T_raw. exposed_stack = [(T_x, T_raw)].
///   3. `x2 = &mut *x` — Ref reborrow from T_x. apply_reborrow_to_stack(Ref) overwrites
///      exposed_stack top: [(T_x, T_x2)]. Alternatively, if the reborrow source is T_x
///      which equals the base_pointer, the stack updates current_borrower to T_x2.
///   4. `_y = &*x2` — shared reborrow; perms = Read, shared_borrower = T_y.
///   5. `*raw` (read via T_raw):
///      - perms = Read: READ path checks shared_borrower first (T_y ≠ T_raw).
///      - Falls back to check_unique_borrower_tag(T_raw, Read):
///        T_x2 is current_borrower; T_raw == prev_borrower? If yes → OK.
///        Or: exposed_stack has an entry with base_pointer T_x and current T_x2;
///        T_raw might match via the exposed_stack lookup.
///      - Either way, HB allows the read.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| T_raw (SRW) is popped by the Unique reborrow `&mut *x` |
/// | TB    | pass    | TB allows reads via ancestors |
/// | HB    | pass    | prev_borrower or exposed_stack entry preserves T_raw |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = &mut 0i32; // T_x
    let raw = x as *mut i32; // Raw retag: T_raw, exposed_stack = [(T_x, T_raw)]
    let x2 = &mut *x; // Ref reborrow: T_x2 displaces T_raw in exposed_stack top
    let _y = &*x2; // shared reborrow: perms = Read, shared_borrower = T_y
    let _val = unsafe { *raw }; // READ via T_raw: HB allows (prev_borrower or stack entry)
    0
}
