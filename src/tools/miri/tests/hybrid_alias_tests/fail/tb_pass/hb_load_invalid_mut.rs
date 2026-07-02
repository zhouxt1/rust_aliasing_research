#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// REGRESSED (2026-07-02): moved here from pass/sb_fail/ after the `RawPointerStack.dead` fix
/// (see COVERAGE.md's "pass/sb_tb_fail case study"). `xref` is never written to (Reserved the
/// whole time in TB terms), so TB's foreign-read-on-Reserved tolerance lets it survive `*xraw`.
/// HB's binary `dead` flag has no such distinction: `*xraw` kills `xref`'s exposed_stack entry
/// unconditionally, so the later reborrow of `xref_ptr` (whose parent tag is now dead) fails
/// with "invalid parent tag" instead of succeeding. Fixing this needs the deferred `activated`
/// field (only kill on parent read once the entry has been self-written).
///
/// Loading a `&mut` from a stack slot after the pointer was "invalidated" by a lower-level
/// raw-pointer access fails in Stacked Borrows but passes in HB and TB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/load_invalid_mut.rs`.
/// That test fails SB. HB and TB both pass.
///
/// ## Original pattern
///
/// ```rust
/// let x = &mut 42;
/// let xraw = x as *mut _;         // T_xraw
/// let xref = &mut *xraw;          // T_xref
/// let xref_in_mem = Box::new(xref); // stores T_xref on heap
/// let _val = *xraw;               // SB: pops T_xref from stack
/// let _val = *xref_in_mem;        // SB ERROR: T_xref was popped
/// ```
///
/// In no_std, `Box::new` is unavailable. We store `xref` in a stack slot via `MaybeUninit`.
///
/// ## SB behavior
///
/// Stack at `*xraw`: [Unique(T_x), SRW(T_xraw), Unique(T_xref)].
/// `*xraw` accesses via T_xraw → pops Unique(T_xref) above it (SB stack semantics).
/// `*xref_in_mem` loads T_xref and reborrows it via Retag → T_xref not in stack → ERROR.
///
/// ## HB behavior
///
/// `xref = &mut *xraw` goes through `apply_reborrow_to_stack(Ref, ...)`:
/// exposed_stack[0] = {base: T_x, current: T_xraw} → current overwritten with T_xref.
/// The old T_xraw is saved for PoloniusAnchor restoration.
///
/// After `xref as *mut i32` (in `xref_slot.as_mut_ptr().write(...)`), exposed_stack grows:
/// [{base:T_x, current:T_xref}, {base:T_xref, current:T_xref_ptr}].
///
/// When xref's PoloniusAnchor fires, exposed_stack[0].current is RESTORED to T_xraw.
/// So `*xraw` succeeds: T_xraw is current_borrower at exposed_stack[0].
///
/// HB has no "pop everything above when accessing via lower entry" semantics.
/// Both T_xraw and T_xref_ptr remain accessible → HB passes.
///
/// ## TB behavior
///
/// TB uses a tree: T_xraw is a child of T_x; T_xref is a child of T_xraw.
/// `*xraw` is a foreign read to T_xref. TB rule: foreign read to Reserved keeps it Reserved.
/// T_xref stays Reserved and can be reactivated → TB passes.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops T_xref; subsequent retag of T_xref fails |
/// | TB    | **pass**| Foreign reads keep Reserved nodes alive; T_xref reactivates |
/// | HB    | **fail**| (post-fix regression) `*xraw` kills xref's entry; later reborrow denied |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 42i32;
    let xraw = &mut x as *mut i32;              // Raw retag: T_xraw pushed to exposed_stack
    let xref: &mut i32 = unsafe { &mut *xraw }; // Ref reborrow: T_xref overwrites top; T_xraw saved

    // Store xref into a stack slot (no_std replacement for Box::new)
    let mut xref_slot = MaybeUninit::<*mut i32>::uninit();
    unsafe { xref_slot.as_mut_ptr().write(xref as *mut i32) };
    // After the line above, xref's PoloniusAnchor fires: T_xraw restored to exposed_stack[0].

    // In SB: pops T_xref from stack (accessing via parent T_xraw) → T_xref invalidated.
    // In HB: T_xraw is current_borrower at exposed_stack[0] (restored by PoloniusAnchor) → OK.
    let _val = unsafe { *xraw };

    // In SB: T_xref was popped by *xraw above, so retagging it fails here → ERROR.
    // In HB: T_xref_ptr still in exposed_stack[1] → reborrow succeeds → pass.
    let loaded: *mut i32 = unsafe { xref_slot.assume_init() };
    let _ref = unsafe { &mut *loaded };

    0
}
