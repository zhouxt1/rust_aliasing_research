#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/cross_tree_update_main.rs (simplified).
// TB: fail — the wildcard reborrow `reb` is rooted in a WILD node; a write through ref2 (a
//     sibling subtree) disables wild/reb, so a subsequent write through reb is UB.
// HB: fail — same mechanism as hb_wildcard_sibling_disable: ref1 and ref2 both reborrow
//     `ptr_base` (RawPtr-source), so ref2 overwrites the `exposed_stack` entry's
//     `current_borrower` in place, erasing ref1's tag from `BorrowerState` entirely. The
//     wildcard (derived from ref1's address) has nothing live to resolve to, so `reb`'s creation
//     itself (a `RawPtr`-source reborrow of the now-untracked wildcard) and/or the final write
//     fails — converging with TB despite HB having no tree structure to drive the invalidation.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;
    let ptr_base = &mut x as *mut u32;
    let ref1 = unsafe { &mut *ptr_base };
    let ref2 = unsafe { &mut *ptr_base };

    let addr = (ref1 as *mut u32).expose_provenance();
    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr);
    let reb = unsafe { &mut *wild };

    // TB: write through ref2 "crosses the tree" and disables reb.
    // HB: ref1's exposed tag is already gone (overwritten by ref2); reb's lineage is broken.
    *ref2 = 99;

    // TB: reb is disabled → fail. HB: reb's underlying tag is not a valid live owner → fail.
    *reb = 1; //~ ERROR: Undefined Behavior

    0
}
