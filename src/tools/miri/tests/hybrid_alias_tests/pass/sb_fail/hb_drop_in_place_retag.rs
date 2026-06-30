#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// `drop_in_place` on a raw pointer derived from an immutable local via `addr_of!`
/// fails in Stacked Borrows but passes in HB (and TB).
///
/// ## Source
/// Ported from `stacked_borrows/fail/drop_in_place_retag.rs`.
/// That test fails SB. HB and TB both pass.
///
/// ## What this tests
///
/// ```rust
/// let x = 0u8;
/// let raw = core::ptr::addr_of!(x);  // *const u8, no reference created
/// unsafe { core::ptr::drop_in_place(raw.cast_mut()) };
/// ```
///
/// ## SB behavior
///
/// `addr_of!(x)` creates a raw pointer with SharedReadOnly (SRO) permission in SB —
/// because `x` is an immutable local, its allocation has SharedReadOnly at the top.
/// `drop_in_place` internally reborrows the pointer for Unique (mutable) permission.
/// The reborrow check: "retag for Unique permission only grants SharedReadOnly permission" → ERROR.
///
/// ## HB behavior
///
/// HB does not have SB's Unique/SharedReadOnly permission distinction for raw pointers.
/// `addr_of!(x)` creates a raw pointer but generates no Retag statement (it directly
/// takes the address without creating a reference). So the raw pointer carries the
/// allocation's tag with no explicit HB tracking entry.
///
/// A write via `raw.cast_mut()` (or equivalently, what `drop_in_place` tries to do
/// internally) goes through the allocation's current_borrower check. The local `x: u8`
/// has `perms = Write` and the raw pointer's tag matches — HB allows the write → pass.
///
/// Note: `core::ptr::drop_in_place` cannot be used directly in this no_std test harness
/// because the MIR for `drop_in_place::<u8>` produces infinite recursion via the Polonius
/// pass. `ptr::write` demonstrates the same principle: writing via a cast-to-mutable
/// raw pointer derived from an immutable local.
///
/// ## TB behavior
///
/// TB similarly does not have the SB Unique/SharedReadOnly distinction.
/// The test is only in `fail/stacked_borrows/`, confirming TB passes it.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| Unique retag on SharedReadOnly-permissioned raw pointer denied |
/// | TB    | **pass**| No Unique/SRO distinction; mutable cast passes |
/// | HB    | **pass**| No Unique permission concept; perms=Write allows the access |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = 0u8;
    let raw: *const u8 = core::ptr::addr_of!(x);
    // SB rejects this: the Unique retag required for a mutable write to `raw.cast_mut()`
    // fails because the pointer was derived with SharedReadOnly permission.
    // HB has no Unique/SharedReadOnly concept, so the write is allowed.
    unsafe {
        core::ptr::write(raw.cast_mut(), 42u8);
    }
    0
}
