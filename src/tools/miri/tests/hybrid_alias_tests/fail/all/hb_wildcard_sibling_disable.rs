#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/single_exposed_disable.rs
// TB: fail — ref1 and ref2 are siblings in the TB tree; *ref2 = 13 disables ref1; read through
//     wild (exposed from ref1) fails because ref1 is Disabled.
// HB: fail — ref1 and ref2 are both `&mut *ptr_base` (RawPtr-source) reborrows of the same raw
//     pointer. The second reborrow (ref2) overwrites the existing `exposed_stack` entry's
//     `current_borrower` in place (the "second reborrow from same base" case in
//     `apply_reborrow_to_stack`) rather than appending a new frame, so ref1's tag is not
//     retained anywhere in `BorrowerState` — not even in `reborrow_chain` (that mechanism only
//     applies to Ref-source reborrows). `resolve_wildcard_tag` therefore finds no live tag
//     matching `exposed_tags`, and the read is denied — converging with TB, though via a
//     different mechanism (tag erasure vs. tree-node disabling).

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;
    let ptr_base = &mut x as *mut u32;
    let ref1 = unsafe { &mut *ptr_base };
    let ref2 = unsafe { &mut *ptr_base };

    let addr = (ref1 as *mut u32).expose_provenance();
    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr);

    // TB: disables ref1. HB: ref2 is already current_borrower; this just writes.
    *ref2 = 13;

    // TB: read through disabled wild → fail.
    // HB: ref1's exposed tag is no longer tracked anywhere live → fail.
    let _v = unsafe { *wild }; //~ ERROR: no exposed tags are currently live

    0
}
