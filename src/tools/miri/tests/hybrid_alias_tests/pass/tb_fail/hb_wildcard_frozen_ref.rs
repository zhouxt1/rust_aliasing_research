#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/single_exposed_local.rs
// TB: fail — wildcard activates ref1, then a parent read freezes ref1; second wildcard
//     write fails because ref1 is Frozen (no write through Frozen in TB).
// HB: pass — this is a genuine, documented HB/TB divergence, not a wildcard-model gap: HB's
//     parent reads never freeze/kill child borrows (see the property table in
//     test_explanation.md). After `let _y = x`, ref1's tag is still current_borrower
//     unchanged, `resolve_wildcard_tag` finds it both live and exposed, and the second write
//     succeeds. Unlike the SB-derived wildcard tests, this divergence survived the
//     `exposed_tags` fix because it stems from HB's read semantics, not from wildcard leniency.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 0;
    let ref1 = unsafe { &mut *(&mut x as *mut u32) };
    let addr = (ref1 as *mut u32).expose_provenance();
    let wild = unsafe { core::ptr::with_exposed_provenance_mut::<u32>(addr) };

    // First write through wildcard — activates ref1 in TB.
    unsafe { wild.write(41) };

    // Parent read — TB: freezes ref1. HB: no freeze on parent read.
    let _y = x;

    // Second write through wildcard.
    // TB: denied (ref1 Frozen). HB: ref1 still current_borrower and exposed — passes.
    unsafe { wild.write(0) };

    0
}
