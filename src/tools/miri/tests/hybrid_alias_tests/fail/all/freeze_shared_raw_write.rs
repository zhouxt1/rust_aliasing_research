#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Interior mutability — write via raw pointer while a plain frozen `&i32` is live.
///
/// ## What this tests
///
/// This is the **counterpart** to pass/test9.rs. The setup is identical — allocate a value,
/// take a raw pointer, create a shared reference, then write through the raw pointer — except
/// here the shared reference is `&i32` (a `Freeze` type) rather than `&UnsafeCell<i32>`
/// (`!Freeze`).
///
/// `i32` is `Freeze`. Under Phase 1 tag inheritance the `!Freeze` branch is not taken,
/// so the allocation transitions normally to `BorrowerPermission::Read`. A write while in
/// `Read` state is unconditionally denied by `check_borrower_tag`. This is UB: the
/// compiler is allowed to assume the value behind a `&i32` does not change.
///
/// ## Why this must keep failing after Phase 1
///
/// Phase 1 dispatches on `ty.is_freeze()` at reborrow time:
/// - `!Freeze` (e.g. `UnsafeCell<i32>`) → inherit `parent_tag`, stay in `Write` → writes OK  (pass/test9)
/// - `Freeze`  (e.g. `i32`)             → `Read` permission → writes UB                       (this test)
///
/// The `Freeze`/`!Freeze` boundary is exactly the semantic line HB must respect; if this
/// test stopped failing it would mean Phase 1 is applying tag inheritance too broadly.
///
/// ## Relation to the SB/TB model
///
/// In Stacked Borrows terms: `&i32` pushes a `SharedReadOnly` item above the raw pointer's
/// Unique tag. A write through the Unique tag below it pops the SRO item from the stack and
/// succeeds — SB does NOT error at the write site. UB in SB is only detected when `frozen`
/// is subsequently READ (its tag has been popped). The final `*frozen` read here ensures the
/// test is unambiguous UB under SB/TB as well as HB.
///
/// HB detects the error earlier (at the write site) because `BorrowerPermission::Read`
/// unconditionally rejects writes regardless of tag identity — no stack to pop.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut val = 0i32;

    // Capture a raw pointer before taking the shared reference.
    let raw: *mut i32 = &mut val as *mut i32;

    // Create a plain frozen shared reference — i32 is Freeze, so this is
    // BorrowerPermission::Read, NOT Cell.
    let frozen: &i32 = unsafe { &*raw };

    // Write through the raw pointer while the frozen shared ref is live.
    // HB: UB here — allocation is in Read permission, write denied.
    // SB: this write pops the SRO item off the stack and succeeds silently;
    //     UB is only triggered by the subsequent read below.
    unsafe { *raw = 42; } //~ ERROR

    // Reading through `frozen` after the write:
    // HB: unreachable (already errored above).
    // SB/TB: UB here — frozen's tag was popped from the stack by the write above.
    let _val = *frozen;

    0
}
